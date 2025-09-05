use crate::{
    cli::Cli,
    ping_client::{PingClient, PingResult},
    ping_surge_client::PingSurgeClient,
    state_machine::{Action, State, StateMachine},
};
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use log::{error, info, warn};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinHandle};
use tonic::transport::{Certificate, Channel, ClientTlsConfig};
use zzping_proto::zzping::{
    ingestion_client::IngestionClient, send_batch_response, GetRecentDataRequest,
    HeartbeatRequest, RawDataRecord, SendBatchRequest,
};

/// A local, serializable version of the `AuthToken` struct.
///
/// This is defined locally to avoid creating a circular dependency, as the collector
/// needs to generate a token for itself, but the canonical `AuthToken` definition
/// lives in `zzping-database` which is only a dev-dependency.
#[derive(Debug, Serialize)]
struct AuthToken<'a> {
    sub: &'a str,
    roles: &'a [&'a str],
}

/// Starts or stops pinger tasks based on the desired state from the database.
///
/// This function compares the list of currently running pinger tasks with the new
/// list of targets received in a `HeartbeatResponse`. It ensures that:
/// - A pinger task is running for every target in the new list.
/// - Any pinger tasks for targets no longer in the list are stopped.
/// - If the collector's state is not `Pinging`, all tasks are stopped.
fn manage_pinger_tasks(
    should_be_pinging: bool,
    new_targets: &[String],
    ping_rate_pps: u64,
    running_tasks: &mut HashMap<IpAddr, JoinHandle<()>>,
    ping_results_tx: &mpsc::Sender<PingResult>,
    cli: &Cli,
) {
    let new_targets_set: HashMap<_, _> = if should_be_pinging {
        new_targets
            .iter()
            .filter_map(|s| s.parse::<IpAddr>().ok())
            .map(|ip| (ip, ()))
            .collect()
    } else {
        HashMap::new() // If not pinging, the target set is empty.
    };

    // Stop tasks that are no longer in the target list
    running_tasks.retain(|target, handle| {
        if !new_targets_set.contains_key(target) {
            info!("Stopping pinger for target {target}");
            handle.abort();
            false
        } else {
            true
        }
    });

    // Start new tasks for new targets
    for target_ip in new_targets_set.keys() {
        if !running_tasks.contains_key(target_ip) {
            info!("Starting pinger for target {target_ip}");
            let ping_client: Arc<dyn PingClient> =
                match PingSurgeClient::new(*target_ip) {
                    Ok(client) => Arc::new(client),
                    Err(e) => {
                        error!("Failed to create ping client for {target_ip}: {e}");
                        continue;
                    }
                };
            let pinger_handle = tokio::spawn(pinger_loop(
                ping_client,
                ping_results_tx.clone(),
                cli.max_in_flight,
                ping_rate_pps,
            ));
            running_tasks.insert(*target_ip, pinger_handle);
        }
    }
}

/// An asynchronous loop that sends pings to a single target at a specified rate.
///
/// This function is spawned as a separate Tokio task for each target IP address.
/// It uses a `Semaphore` to limit the number of concurrent, in-flight pings,
/// preventing the system from being overwhelmed.
///
/// Each successful ping dispatch results in a `PingResult` being sent back to the
/// main `runner` loop via the `ping_tx` channel.
async fn pinger_loop(
    ping_client: Arc<dyn PingClient>,
    ping_tx: mpsc::Sender<PingResult>,
    max_in_flight: usize,
    rate: u64,
) {
    if rate == 0 {
        warn!("Ping rate is 0, pinger loop will not run.");
        return;
    }
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_in_flight));
    let start_time = Instant::now();
    let mut sequence_idx: u16 = 0;
    let mut interval = tokio::time::interval(Duration::from_secs_f64(1.0 / rate as f64));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        interval.tick().await;
        if let Ok(permit) = semaphore.clone().try_acquire_owned() {
            ping_client
                .ping(
                    sequence_idx,
                    ping_tx.clone(),
                    permit,
                    start_time,
                    Instant::now(),
                )
                .await;
            sequence_idx = sequence_idx.wrapping_add(1);
        }
    }
}

/// An asynchronous loop that collects ping results and sends them to the database in batches.
///
/// This function runs in a separate Tokio task. It receives `PingResult`s from
/// all active `pinger_loop` tasks, buffers them, and sends them to the database
/// every 1 second.
///
/// It also implements the client-side logic for the ACK/DESYNC protocol. If the
/// database responds with `DESYNC`, this loop will rewind its buffer to the last
/// known-good state and immediately retry sending the batch.
pub async fn batch_sender_loop(
    mut client: IngestionClient<Channel>,
    collector_uuid: String,
    mut ping_results_rx: mpsc::Receiver<PingResult>,
    token: String,
) {
    let mut buffer: VecDeque<RawDataRecord> = VecDeque::new();
    let mut last_acked_nanos = 0;
    let mut interval = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            Some(ping_result) = ping_results_rx.recv() => {
                buffer.push_back(RawDataRecord {
                    sent_nanos: ping_result.sent_nanos,
                    rtt_nanos: ping_result.rtt.map_or(u64::MAX, |rtt| rtt.as_nanos() as u64),
                });
            }
            _ = interval.tick() => {
                let mut needs_immediate_retry = true;
                while needs_immediate_retry {
                    needs_immediate_retry = false;

                    if buffer.is_empty() {
                        break;
                    }

                    let records_to_send: Vec<_> = buffer.iter().cloned().collect();
                    let mut request = tonic::Request::new(SendBatchRequest {
                        collector_uuid: collector_uuid.clone(),
                        records: records_to_send,
                        collector_believes_last_acked_nanos: last_acked_nanos,
                    });
                    request.metadata_mut().insert("authorization", format!("Bearer {}", token).parse().unwrap());

                    match client.send_batch(request).await {
                        Ok(response) => {
                            let response = response.into_inner();
                            match send_batch_response::Status::try_from(response.status) {
                                Ok(send_batch_response::Status::Ok) => {
                                    info!("Batch sent successfully. New acked_nanos: {}", response.database_confirms_last_acked_nanos);
                                    last_acked_nanos = response.database_confirms_last_acked_nanos;
                                    buffer.retain(|r| r.sent_nanos > last_acked_nanos);
                                },
                                Ok(send_batch_response::Status::Desync) => {
                                    warn!("Received DESYNC from server. DB confirms acked_nanos: {}. Rewinding buffer.", response.database_confirms_last_acked_nanos);
                                    last_acked_nanos = response.database_confirms_last_acked_nanos;
                                    buffer.retain(|r| r.sent_nanos > last_acked_nanos);
                                    needs_immediate_retry = true;
                                }
                                Err(_) => {
                                    error!("Unknown status in SendBatchResponse: {}", response.status);
                                }
                            }
                        },
                        Err(e) => {
                            error!("send_batch RPC failed: {e}. Data will be retried.");
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// The main entry point and runtime loop for the `zzping-collector` service.
///
/// This function orchestrates the entire lifecycle of the collector:
/// 1.  Parses command-line arguments.
/// 2.  Establishes a gRPC connection to the database, with a retry loop.
/// 3.  Spawns the `batch_sender_loop` as a background task.
/// 4.  Enters the main heartbeat loop, where it periodically calls the `heartbeat`
///     RPC on the database to get its configuration and role.
/// 5.  Uses a `StateMachine` to manage its own operational state (`Pinging`,
///     `Standby`, `Shutdown`).
/// 6.  Based on the state, it uses `manage_pinger_tasks` to start or stop the
///     individual pinger tasks for each target IP.
/// 7.  Handles `Shutdown` commands to exit gracefully.
pub async fn run() -> Result<()> {
    use clap::Parser;
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()
        .ok();
    let cli = Arc::new(Cli::parse());
    info!("Starting zzping-collector v{}", env!("CARGO_PKG_VERSION"));
    info!("Source hostname: {}", cli.source_hostname);
    info!("Database address: {}", cli.database_addr);

    let ca_cert = tokio::fs::read("ca.pem")
        .await
        .context("Unable to read ca_cert as ./ca.pem")?;
    let retry_delay = Duration::from_secs(5);
    let pid = std::process::id() as u64;

    loop {
        info!("Attempting to connect to gRPC server...");
        let host = if cli.database_addr.starts_with("https://") {
            cli.database_addr.strip_prefix("https://").unwrap()
        } else {
            &cli.database_addr
        };
        let domain_name = host.split(':').next().unwrap();
        let ca = if cli.database_addr.starts_with("https://") {
            Some(Certificate::from_pem(ca_cert.clone()))
        } else {
            None
        };
        let tls_config = ca.as_ref().map(|ca| {
            ClientTlsConfig::new()
                .domain_name(domain_name)
                .ca_certificate(ca.clone())
        });
        let channel_builder = Channel::from_shared(cli.database_addr.clone()).unwrap();
        let channel = if let Some(tls) = tls_config {
            channel_builder.tls_config(tls).unwrap()
        } else {
            channel_builder
        };

        match channel.connect().await {
            Ok(channel) => {
                let mut client = IngestionClient::new(channel.clone());
                info!("gRPC client connected. Starting main loop.");

                let (ping_results_tx, ping_results_rx) = mpsc::channel(1000);
                let mut running_tasks: HashMap<IpAddr, JoinHandle<()>> = HashMap::new();
                let mut state_machine = StateMachine::new(cli.source_hostname.clone());

                let token = {
                    let auth_token = AuthToken {
                        sub: &cli.source_hostname,
                        roles: &["collector"],
                    };
                    let json = serde_json::to_string(&auth_token).unwrap();
                    general_purpose::STANDARD.encode(json)
                };
                let batch_sender_handle = tokio::spawn(batch_sender_loop(
                    IngestionClient::new(channel.clone()),
                    cli.source_hostname.clone(),
                    ping_results_rx,
                    token.clone(),
                ));

                let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(1));
                loop {
                    heartbeat_interval.tick().await;
                    let mut request = tonic::Request::new(HeartbeatRequest {
                        collector_uuid: cli.source_hostname.clone(),
                        pid,
                    });
                    request.metadata_mut().insert("authorization", format!("Bearer {}", token).parse().unwrap());

                    match client.heartbeat(request).await {
                        Ok(response) => {
                            let response = response.into_inner();
                            let action = state_machine.handle_heartbeat_response(&response);

                            if let Some(Action::SeedBuffer) = action {
                                info!("Seeding buffer by calling get_recent_data...");
                                let request = tonic::Request::new(GetRecentDataRequest {
                                    collector_uuid: cli.source_hostname.clone(),
                                    lookback_seconds: 3600, // 1 hour
                                });
                                match client.get_recent_data(request).await {
                                    Ok(data) => info!("Successfully seeded buffer with {} records.", data.into_inner().records.len()),
                                    Err(e) => warn!("Failed to seed buffer: {e}"),
                                }
                            }

                            if state_machine.current_state == State::Shutdown {
                                info!("Received SHUTDOWN command. Exiting.");
                                batch_sender_handle.abort();
                                for handle in running_tasks.values() {
                                    handle.abort();
                                }
                                return Ok(());
                            }

                            let should_be_pinging = state_machine.current_state == State::Pinging;
                            manage_pinger_tasks(
                                should_be_pinging,
                                &response.targets,
                                response.ping_rate_pps,
                                &mut running_tasks,
                                &ping_results_tx,
                                &cli,
                            );
                        }
                        Err(e) => {
                            error!("Heartbeat failed: {e}. Reconnecting...");
                            batch_sender_handle.abort();
                            for (_, handle) in running_tasks {
                                handle.abort();
                            }
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to connect to gRPC server: {e}. Retrying in {retry_delay:?}.");
                tokio::time::sleep(retry_delay).await;
            }
        }
    }
}
