use crate::{
    cli::Cli, connection_manager, ping_client::PingClient, ping_surge_client::PingSurgeClient,
};
use anyhow::Result;
use log::{error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::{net::TcpStream, task::JoinHandle};

/// The core logic for pinging a single target and reporting to the database.
///
/// This function is spawned in its own asynchronous task for each target.
/// It is given a `PingClient` and then enters an infinite loop to maintain a
/// connection to the database. If the connection is lost, it will automatically
/// try to reconnect after a 5-second delay.
pub async fn ping_target_loop(cli: Arc<Cli>, ping_client: Arc<dyn PingClient>) -> Result<()> {
    let target = ping_client.target();
    info!("Pinging target: {target}");

    // The main loop of the collector is designed for resilience. It will continuously
    // try to connect to the database, and if the connection is ever lost, it will
    // simply re-enter this loop and try to connect again.
    loop {
        info!(
            "[{}] Attempting to connect to database at {}",
            target, cli.database_addr
        );
        match TcpStream::connect(&cli.database_addr).await {
            Ok(stream) => {
                info!("[{target}] Successfully connected to database.");
                // Once connected, hand off to the connection manager, which will run
                // until the connection is lost.
                if let Err(e) =
                    connection_manager::handle_connection(stream, ping_client.clone(), cli.clone())
                        .await
                {
                    error!(
                        "[{target}] Error during connection handling: {e}. Reconnecting..."
                    );
                }
            }
            Err(e) => {
                error!(
                    "[{target}] Failed to connect to database: {e}. Retrying in 5 seconds."
                );
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

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

    let mut handles: Vec<JoinHandle<Result<()>>> = Vec::new();

    for target in cli.targets.clone() {
        let cli = Arc::clone(&cli);
        let ping_client: Arc<dyn PingClient> = Arc::new(PingSurgeClient::new(target)?);
        let handle = tokio::spawn(ping_target_loop(cli, ping_client));
        handles.push(handle);
    }

    // Wait for all tasks to complete. In practice, these tasks run indefinitely,
    // so this await will likely never return. This is acceptable for a long-running
    // service. If one of the tasks panics, it will be caught here.
    for handle in handles {
        handle.await??;
    }

    Ok(())
}
