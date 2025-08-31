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

use crate::ping_surge_client::PingSurgeClient;
use anyhow::Result;
use clap::Parser;
use log::{error, info};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;

mod connection_manager;
mod ping_client;
mod ping_surge_client;

#[cfg(test)]
mod ping_mock_client;

/// A high-frequency ICMP pinger that sends results to a zzping-database server.
#[derive(Parser, Debug)]
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

/// The main function for the collector service.
///
/// Initializes logging, parses CLI arguments, and enters the main connection loop.
#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();
    let cli = Arc::new(Cli::parse());
    info!("Starting zzping-collector");
    info!("Source hostname: {}", cli.source_hostname);
    info!("Targets: {:?}", cli.targets);
    // TODO: Implement logic to handle multiple targets
    // For now, we only ping the first target
    if cli.targets.is_empty() {
        panic!("At least one target must be specified");
    }
    let primary_target = cli.targets[0];
    info!("Primary target: {}", primary_target);
    info!("Rate: {} pps", cli.rate);
    info!("Database address: {}", cli.database_addr);
    info!("Max in-flight: {}", cli.max_in_flight);

    let ping_client = Arc::new(PingSurgeClient::new(primary_target)?);

    // The main loop of the collector is designed for resilience. It will continuously
    // try to connect to the database, and if the connection is ever lost, it will
    // simply re-enter this loop and try to connect again.
    loop {
        info!("Attempting to connect to database at {}", cli.database_addr);
        match TcpStream::connect(&cli.database_addr).await {
            Ok(stream) => {
                info!("Successfully connected to database.");
                // Once connected, hand off to the connection manager, which will run
                // until the connection is lost.
                if let Err(e) = connection_manager::handle_connection(
                    stream,
                    Arc::clone(&ping_client),
                    cli.clone(),
                )
                .await
                {
                    error!("Error during connection handling: {e}. Reconnecting...");
                }
            }
            Err(e) => {
                error!("Failed to connect to database: {e}. Retrying in 5 seconds.");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
