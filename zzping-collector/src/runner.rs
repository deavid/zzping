use crate::{
    cli::Cli, ping_client::PingClient, ping_surge_client::PingSurgeClient,
    target_manager::run_target_manager,
};
use anyhow::{Context, Result};
use log::info;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

/// The main function for the collector service.
///
/// Initializes logging, parses CLI arguments, and enters the main connection loop.
pub async fn run() -> Result<()> {
    use clap::Parser;
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()
        .ok();
    let cli = Arc::new(Cli::parse());
    info!("Starting zzping-collector");
    info!("Source hostname: {}", cli.source_hostname);
    info!("Targets: {:?}", cli.targets);
    info!("Rate: {} pps", cli.rate);
    info!("Database address: {}", cli.database_addr);
    info!("Max in-flight: {}", cli.max_in_flight);

    if cli.targets.is_empty() {
        panic!("At least one target must be specified");
    }

    let ca_cert = tokio::fs::read("ca.pem")
        .await
        .context("Unable to read ca_cert as ./ca.pem")?;

    let mut handles: Vec<JoinHandle<()>> = Vec::new();

    for target in cli.targets.clone() {
        let cli = Arc::clone(&cli);
        let ping_client: Arc<dyn PingClient> = Arc::new(PingSurgeClient::new(target)?);
        let (ping_tx, ping_rx) = tokio::sync::mpsc::channel(100);

        let pinger_handle = {
            let ping_client = ping_client.clone();
            let cli_clone = cli.clone();
            tokio::spawn(async move {
                let semaphore = Arc::new(tokio::sync::Semaphore::new(cli_clone.max_in_flight));
                let start_time = Instant::now();
                let mut sequence_idx: u16 = 0;
                let mut interval = tokio::time::interval(std::time::Duration::from_secs_f64(
                    1.0 / cli_clone.rate as f64,
                ));
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
            })
        };
        handles.push(pinger_handle);

        let manager_handle = tokio::spawn(run_target_manager(
            ca_cert.clone(),
            cli.database_addr.clone(),
            cli.source_hostname.clone(),
            target,
            cli.auth_token.clone(),
            ping_rx,
            Duration::from_secs(5),
        ));
        handles.push(manager_handle);
    }

    for handle in handles {
        handle.await?;
    }

    Ok(())
}
