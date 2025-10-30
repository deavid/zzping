//! Session handler for per-collector sessions.

use crate::{
    collector_service::CachedIntent,
    database_client::DatabaseClientTrait,
    task_supervisor::{HealthReport, SupervisorConfig},
};
use anyhow::Result;
use log::{error, info, warn};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{collections::HashSet, time::Duration};
use tokio::sync::{mpsc, watch};
use zzping_proto::zzping::{
    CollectorRole, Command, CommandRequest, HeartbeatRequest, HeartbeatResponse,
    command::CommandType,
};

/// An internal message to the state manager loop.
#[derive(Debug)]
enum SessionUpdate {
    FromHeartbeat(HeartbeatResponse),
    FromCommand(Command),
}

/// An ephemeral task that manages all gRPC communication for the duration of
/// a single, healthy connection. It dies gracefully on any network error.
pub struct SessionHandler {
    /// The gRPC client for this session.
    client: Arc<dyn DatabaseClientTrait>,
    /// The sender for broadcasting configuration updates.
    config_tx: watch::Sender<Option<SupervisorConfig>>,
    /// The UUID of this collector.
    collector_uuid: String,
    /// A receiver for the latest health report from the service.
    health_rx: watch::Receiver<HealthReport>,
    /// A sender for persisting the latest config.
    persistence_tx: mpsc::Sender<CachedIntent>,
    /// A sender to notify TaskSupervisor of fsync acknowledgments from DB.
    fsync_tx: mpsc::Sender<u64>,
    /// Whether to use mock ping clients (for testing).
    use_mock_ping_client: bool,
    cache_file_path: Option<PathBuf>,
}

impl SessionHandler {
    /// Creates a new `SessionHandler`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: Arc<dyn DatabaseClientTrait>,
        config_tx: watch::Sender<Option<SupervisorConfig>>,
        collector_uuid: String,
        health_rx: watch::Receiver<HealthReport>,
        persistence_tx: mpsc::Sender<CachedIntent>,
        fsync_tx: mpsc::Sender<u64>,
        use_mock_ping_client: bool,
        cache_file_path: Option<PathBuf>,
    ) -> Self {
        Self {
            client,
            config_tx,
            collector_uuid,
            health_rx,
            persistence_tx,
            fsync_tx,
            use_mock_ping_client,
            cache_file_path,
        }
    }
    /// Runs the `SessionHandler`'s main loops.
    pub async fn run(self) -> Result<()> {
        info!("SessionHandler started.");

        let (update_tx, update_rx) = mpsc::channel(10);
        info!(
            "Created channel: update_tx addr={:p}, update_rx addr={:p}",
            &update_tx, &update_rx
        );
        let client_clone1 = self.client.clone();
        let client_clone2 = self.client.clone();
        let last_processed_command = Arc::new(AtomicU64::new(0));

        let mut heartbeat_handle = tokio::spawn(Self::run_heartbeat_loop(
            client_clone1,
            self.health_rx.clone(),
            self.collector_uuid.clone(),
            update_tx.clone(),
            last_processed_command.clone(),
            self.fsync_tx.clone(),
        ));
        let mut command_handle = tokio::spawn(Self::run_command_loop(
            client_clone2,
            self.collector_uuid,
            update_tx,
        ));
        let mut state_manager_handle = tokio::spawn(Self::run_state_manager_loop(
            update_rx,
            self.config_tx,
            self.persistence_tx,
            self.use_mock_ping_client,
            last_processed_command.clone(),
            self.cache_file_path,
        ));

        // The session ends if any of the core loops finishes. When that
        // happens we abort the remaining loops to ensure the session returns
        // promptly (avoids background tasks preventing immediate shutdown
        // in tests).
        let result = tokio::select! {
            res = &mut heartbeat_handle => {
                warn!("Heartbeat loop ended.");
                // Abort the other tasks immediately
                command_handle.abort();
                state_manager_handle.abort();
                res
            }
            res = &mut command_handle => {
                warn!("Command stream loop ended.");
                heartbeat_handle.abort();
                state_manager_handle.abort();
                res
            }
            res = &mut state_manager_handle => {
                warn!("State manager loop ended.");
                heartbeat_handle.abort();
                command_handle.abort();
                res
            }
        };

        // Propagate the inner join result. If the task itself returned an
        // error, propagate that; if the join failed, convert it to an error.
        match result {
            Ok(inner) => match inner {
                Ok(_) => (),
                Err(e) => return Err(e),
            },
            Err(join_err) => return Err(anyhow::anyhow!("Session task join failed: {join_err}")),
        }

        Ok(())
    }

    async fn run_state_manager_loop(
        mut update_rx: mpsc::Receiver<SessionUpdate>,
        config_tx: watch::Sender<Option<SupervisorConfig>>,
        persistence_tx: mpsc::Sender<CachedIntent>,
        use_mock_ping_client: bool,
        last_processed_command: Arc<AtomicU64>,
        cache_file_path: Option<PathBuf>,
    ) -> Result<()> {
        info!(
            "State manager loop starting with update_rx addr={:p}",
            &update_rx
        );
        let mut current_config = SupervisorConfig {
            targets: HashSet::new(),
            ping_rate_pps: 0,
            role: CollectorRole::Standby,
            use_mock_ping_client,
            swap_at_nanos: None,
        };
        // Try to load the last known config from disk, if available.
        if let Some(cache_path) = &cache_file_path
            && let Ok(data) = std::fs::read_to_string(cache_path)
        {
            if let Ok(intent) = ron::from_str::<CachedIntent>(&data) {
                current_config.targets = intent.targets.clone();
                current_config.ping_rate_pps = intent.ping_rate_pps;
                info!(
                    "Loaded cached intent from disk: targets={} rate={}",
                    current_config.targets.len(),
                    current_config.ping_rate_pps
                );
                // Do NOT broadcast the cached intent immediately here. We set the
                // in-memory current_config so the supervisor can be later reconciled
                // when a heartbeat or command arrives. Broadcasting immediately
                // causes tests that expect the heartbeat-derived config to fail.
            } else {
                info!("Failed to parse cached intent; continuing without cached intent.");
            }
        }

        info!("State manager loop starting...");
        while let Some(update) = update_rx.recv().await {
            info!("State manager received update: {update:?}");
            match update {
                SessionUpdate::FromHeartbeat(response) => {
                    current_config.role =
                        CollectorRole::try_from(response.role).unwrap_or(CollectorRole::Standby);
                    current_config.targets = response
                        .targets
                        .into_iter()
                        .filter_map(|s| s.parse().ok())
                        .collect();
                    current_config.ping_rate_pps = response.ping_rate_pps;
                    // Propagate swap_at_nanos if present
                    if response.swap_at_nanos > 0 {
                        // Use Some to indicate a scheduled swap time
                        // Note: TaskSupervisor will interpret this as guidance for scheduling.
                        current_config.swap_at_nanos = Some(response.swap_at_nanos);
                    } else {
                        current_config.swap_at_nanos = None;
                    }
                }
                SessionUpdate::FromCommand(command) => {
                    // Track last processed command id for heartbeat reporting
                    // Generated proto usually provides a command_id field.
                    let cmd_id = command.command_id;
                    last_processed_command.store(cmd_id, Ordering::SeqCst);

                    if let Some(command_type) = command.command_type {
                        match command_type {
                            CommandType::ChangeRole(role) => {
                                current_config.role =
                                    CollectorRole::try_from(role).unwrap_or(current_config.role);
                            }
                            CommandType::PrepareToSwap(prep) => {
                                // PrepareToSwap carries a desired role and a swap_at_nanos timestamp.
                                // Update the in-memory config role so the supervisor and workers
                                // can prepare for the scheduled swap. The exact timing/coordination
                                // is handled elsewhere; here we simply apply the requested role
                                // and log the scheduled swap time for observability.
                                let requested_role = CollectorRole::try_from(prep.role)
                                    .unwrap_or(current_config.role);
                                current_config.role = requested_role;
                                current_config.swap_at_nanos = Some(prep.swap_at_nanos);
                                info!(
                                    "Received PrepareToSwap: set role to {:?} (swap_at_nanos={:?})",
                                    current_config.role, current_config.swap_at_nanos
                                );
                            }
                        }
                    }
                }
            }
            info!("Broadcasting new config: {current_config:?}");
            if config_tx.send(Some(current_config.clone())).is_err() {
                warn!("Config channel closed, continuing without supervisor subscription.");
                // Do not break; allow the state manager to continue processing commands
                // and other updates even if the consumer of configs has dropped.
            }

            // Also send the config to be persisted.
            let intent = CachedIntent {
                targets: current_config.targets.clone(),
                ping_rate_pps: current_config.ping_rate_pps,
            };
            if persistence_tx.send(intent).await.is_err() {
                warn!("Persistence channel closed; continuing without persistence.");
                // Continue even if persistence task is not available.
            }
        }
        info!("State manager loop exiting - update_rx.recv() returned None");
        Ok(())
    }

    async fn run_heartbeat_loop(
        client: Arc<dyn DatabaseClientTrait>,
        health_rx: watch::Receiver<HealthReport>,
        collector_uuid: String,
        update_tx: mpsc::Sender<SessionUpdate>,
        last_processed_command: Arc<AtomicU64>,
        fsync_tx: mpsc::Sender<u64>,
    ) -> Result<()> {
        info!(
            "Heartbeat loop starting with update_tx addr={:p}",
            &update_tx
        );
        // FIXME: This interval must be configurable externally, specially for unit tests!
        let mut interval = tokio::time::interval(if cfg!(test) {
            Duration::from_millis(1)
        } else {
            Duration::from_millis(100)
        });
        loop {
            interval.tick().await;
            let health_report = health_rx.borrow().clone();

            // Read the last processed command id from the shared atomic each tick so
            // the heartbeat reflects the most-recently-processed command.
            let last_processed_command_id = last_processed_command.load(Ordering::SeqCst);

            let request = HeartbeatRequest {
                collector_uuid: collector_uuid.clone(),
                pid: std::process::id() as u64,
                current_role: health_report.role.into(),
                buffer_record_count: health_report.total_buffer_size as u64,
                last_fatal_error: health_report.fatal_errors.join(", "),
                last_processed_command_id,
            };

            // Add timeout to heartbeat request
            let heartbeat_result = tokio::time::timeout(
                Duration::from_millis(500), // Timeout for heartbeat
                client.heartbeat(request),
            )
            .await;

            match heartbeat_result {
                Ok(result) => match result {
                    Ok(response) => {
                        let resp = response.into_inner();
                        // If the response includes a last_fsynced_received_nanos field, notify supervisor
                        if resp.last_fsynced_received_nanos > 0 {
                            let _ = fsync_tx.send(resp.last_fsynced_received_nanos).await;
                        }
                        if update_tx
                            .send(SessionUpdate::FromHeartbeat(resp))
                            .await
                            .is_err()
                        {
                            info!("State manager disconnected, heartbeat loop shutting down.");
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Heartbeat RPC failed: {e}. Session ending.");
                        return Err(e);
                    }
                },
                Err(_) => {
                    error!("Heartbeat request timed out - connection likely failed.");
                    return Err(anyhow::anyhow!("Heartbeat timeout"));
                }
            }
        }
        Ok(())
    }

    async fn run_command_loop(
        client: Arc<dyn DatabaseClientTrait>,
        collector_uuid: String,
        update_tx: mpsc::Sender<SessionUpdate>,
    ) -> Result<()> {
        info!("Command loop starting with update_tx addr={:p}", &update_tx);
        let request = CommandRequest { collector_uuid };
        let mut stream = client.subscribe_to_commands(request).await?.into_inner();
        info!("Successfully subscribed to command stream.");

        loop {
            // Add timeout to detect if stream is stuck
            let message_result = tokio::time::timeout(
                if cfg!(test) {
                    // Short timeout in tests
                    Duration::from_millis(10)
                } else {
                    Duration::from_millis(1000)
                },
                stream.message(),
            )
            .await;

            match message_result {
                Ok(message_result) => match message_result? {
                    Some(command) => {
                        info!("Received command: {command:?}");
                        info!("About to send SessionUpdate::FromCommand to state manager...");
                        if update_tx
                            .send(SessionUpdate::FromCommand(command))
                            .await
                            .is_err()
                        {
                            info!("State manager disconnected, command loop shutting down.");
                            break;
                        }
                        info!("Successfully sent SessionUpdate::FromCommand to state manager.");
                    }
                    None => {
                        info!("Command stream ended (received None).");
                        break;
                    }
                },
                Err(_) => {
                    // Timeout occurred - this might indicate connection issues
                    info!("Command stream message timeout - checking if we should exit.");

                    // Check if the update_tx is still alive (state manager still running)
                    if update_tx.is_closed() {
                        info!("Update channel closed, command loop shutting down.");
                        break;
                    }

                    // Continue the loop to try again
                    continue;
                }
            }
        }

        warn!("Command stream from database was closed.");
        Ok(())
    }
}
