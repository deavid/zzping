//! Top-level design notes for the collector library.
//!
//! This crate provides composable components for implementing the collector.
//! It emphasizes correctness, simplicity, and modularity.
//!
//! Key principles:
//! - The orchestrator drives configuration and state transitions.
//! - Components react to orchestrator decisions, ensuring a clear separation of concerns.
//! - Focused optimizations are preferred over global complexity.
//!
pub mod batch_submitter;
pub mod cli;
pub mod collector_service;
pub mod config;
pub mod connection_manager;
pub mod database_client;
pub mod ping_client;
pub mod ping_mock_client;
pub mod pinger;
pub mod ping_surge_client;
pub mod session_handler;
pub mod state_machine;
pub mod target_worker;
pub mod task_supervisor;

use anyhow::Result;
use clap::Parser;
use log::info;
use std::net::{SocketAddr, TcpListener};
use crate::{cli::Cli, collector_service::CollectorService, config::Config};

/// The main entry point for the collector's library code from the binary.
pub async fn run() -> Result<()> {
    // Initialize logging for operational visibility
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()
        .ok();

    info!("Starting zzping-collector...");

    // Parse command-line arguments to get the config path
    let cli = Cli::parse();
    run_with_config_path(cli.config).await
}


/// Runs the collector with a specific configuration path.
/// This function contains the core logic of the application,
/// making it testable.
pub async fn run_with_config_path(config_path: String) -> Result<()> {
    let service = bootstrap_collector(config_path)?;
    // The lock is now owned by the service, so we just need to run it.
    service.run().await
}

/// Creates all the core components of the collector, including acquiring the
/// TCP port lock, but does not start the main event loop. This makes the
/// startup process more easily testable.
pub fn bootstrap_collector(config_path: String) -> Result<CollectorService> {
    // Load configuration
    let config = Config::load(&config_path)?;
    info!("Configuration loaded from {}", &config_path);

    // Acquire the local TCP port lock for mutual exclusion
    let lock_addr: SocketAddr = "127.0.0.1:7879".parse()?;
    let lock = match TcpListener::bind(lock_addr) {
        Ok(listener) => {
            info!("Successfully acquired TCP port lock on {lock_addr}");
            listener
        }
        Err(e) => {
            anyhow::bail!(
                "Failed to acquire TCP port lock on {}: {}. Another instance may be running.",
                lock_addr,
                e
            );
        }
    };

    // Create the collector service, passing ownership of the lock to it.
    let service = CollectorService::new(config, lock)?;
    Ok(service)
}
