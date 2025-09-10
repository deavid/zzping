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
pub mod ping_surge_client;
pub mod pinger;
pub mod session_handler;

pub mod target_worker;
pub mod task_supervisor;

use crate::{cli::Cli, collector_service::{CollectorService, CachedIntent}, config::Config};
use anyhow::Result;
use clap::Parser;
use log::info;
use std::net::TcpListener;
use tokio::sync::mpsc;

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
    bootstrap_collector_with_port(config_path, None, None)
}

use crate::task_supervisor::SupervisorShutdown;

/// Test-only bootstrap function that allows injecting shutdown commands.
pub fn bootstrap_collector_for_test(
    config_path: String,
    test_shutdown_tx_sender: mpsc::Sender<mpsc::Sender<SupervisorShutdown>>,
) -> Result<CollectorService> {
    let config = Config::load(&config_path)?;
    let lock = TcpListener::bind("127.0.0.1:0")?;
    CollectorService::new_for_test(config, lock, Some(test_shutdown_tx_sender), None)
}

/// Test helper that also accepts a worker_count_tx to observe worker counts.
pub fn bootstrap_collector_for_test_with_worker_tx(
    config_path: String,
    test_shutdown_tx_sender: mpsc::Sender<mpsc::Sender<SupervisorShutdown>>,
    worker_count_tx: Option<mpsc::Sender<usize>>,
) -> Result<CollectorService> {
    let config = Config::load(&config_path)?;
    let lock = TcpListener::bind("127.0.0.1:0")?;
    // Use default interval
    let svc = CollectorService::new_for_test_with_interval_and_worker_tx(
        config,
        lock,
        Some(test_shutdown_tx_sender),
        1000,
        None,
        worker_count_tx,
    )?;
    Ok(svc)
}

/// Test helper that allows specifying a custom health interval (ms).
pub fn bootstrap_collector_for_test_with_interval(
    config_path: String,
    test_shutdown_tx_sender: mpsc::Sender<mpsc::Sender<SupervisorShutdown>>,
    health_interval_ms: u64,
) -> Result<CollectorService> {
    let config = Config::load(&config_path)?;
    let lock = TcpListener::bind("127.0.0.1:0")?;
    let svc = CollectorService::new_for_test_with_interval(
        config,
        lock,
        Some(test_shutdown_tx_sender),
        health_interval_ms,
        None,
    )?;
    Ok(svc)
}

/// Creates all the core components of the collector with a specific port.
/// Used for testing the port lock mechanism.
pub fn bootstrap_collector_with_port(
    config_path: String,
    port: Option<u16>,
    _unused_test_param: Option<()>,
) -> Result<CollectorService> {
    // Load configuration
    let config = Config::load(&config_path)?;
    info!("Configuration loaded from {config_path}");

    // Acquire the local TCP port lock for mutual exclusion
    let lock = if let Some(port) = port {
        TcpListener::bind(format!("127.0.0.1:{port}"))?
    } else {
        TcpListener::bind("127.0.0.1:0")?
    };

    // Create the collector service, passing ownership of the lock to it.
    let cached_intent = if let Ok(data) = std::fs::read_to_string("last_intent.ron") {
        ron::from_str::<CachedIntent>(&data).ok()
    } else {
        None
    };
    let service = CollectorService::new(config, lock, cached_intent)?;
    Ok(service)
}
