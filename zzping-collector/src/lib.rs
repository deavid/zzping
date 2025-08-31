//! The main entry point for the `zzping-collector` service.
//!
//! This service is responsible for performing high-frequency ICMP pings against
//! a specified target and sending the results to a `zzping-database` instance
//! for storage and analysis.
//!
//! # Main Logic
//! 1. Parses command-line arguments to get the target IP, ping rate, and database address.
//! 2. Enters an infinite loop to maintain a persistent connection to the database.
//! 3. Inside the loop, it attempts to connect. If it fails, it waits 5 seconds and retries.
//! 4. Once connected, it hands off the stream to the `connection_manager` to handle
//!    the pinging and data transmission for the life of the connection.

pub use crate::ping_client::PingClient;
pub use crate::ping_surge_client::PingSurgeClient;
pub use anyhow::Result;
pub use clap::Parser;
use log::{error, info};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::{net::TcpStream, task::JoinHandle};

pub mod connection_manager;
pub mod ping_client;
pub mod ping_surge_client;

pub mod ping_mock_client;

/// A high-frequency ICMP pinger that sends results to a zzping-database server.
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// The IP addresses to ping. Multiple targets can be specified.
    #[arg(long)]
    pub targets: Vec<IpAddr>,

    /// The hostname of this collector instance.
    #[arg(long)]
    pub source_hostname: String,

    /// The number of pings to send per second.
    #[arg(long)]
    pub rate: u64,

    /// The address of the zzping-database server.
    #[arg(long, default_value = "127.0.0.1:7878")]
    pub database_addr: String,

    /// The maximum number of pings in flight at any given time.
    ///
    /// This acts as a backpressure mechanism. If the network is slow or pings are
    /// timing out, this limit prevents the collector from flooding the network with
    /// an ever-increasing number of outstanding packets. A low number is recommended
    /// to avoid causing a denial-of-service-like event on the target host.
    #[arg(long, default_value = "3")]
    pub max_in_flight: usize,
}

/// The core logic for pinging a single target and reporting to the database.
///
/// This function is spawned in its own asynchronous task for each target.
/// It is given a `PingClient` and then enters an infinite loop to maintain a
/// connection to the database. If the connection is lost, it will automatically
/// try to reconnect after a 5-second delay.
pub async fn ping_target_loop(cli: Arc<Cli>, ping_client: Arc<dyn PingClient>) -> Result<()> {
    let target = ping_client.target();
    info!("Pinging target: {}", target);

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
                info!("[{}] Successfully connected to database.", target);
                // Once connected, hand off to the connection manager, which will run
                // until the connection is lost.
                if let Err(e) = connection_manager::handle_connection(
                    stream,
                    ping_client.clone(),
                    cli.clone(),
                )
                .await
                {
                    error!(
                        "[{}] Error during connection handling: {e}. Reconnecting...",
                        target
                    );
                }
            }
            Err(e) => {
                error!(
                    "[{}] Failed to connect to database: {e}. Retrying in 5 seconds.",
                    target
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
