//! Minimal entry point for the zzping collector.
//!
//! This binary initializes the orchestrator, which manages all collector
//! operations. The main function is intentionally simple to facilitate
//! testing and maintainability.

use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use zzping_collector::{cli::Cli, orchestrator::CollectorOrchestrator};

/// Main entry point for the zzping collector executable.
///
/// This function sets up the asynchronous runtime environment and
/// initializes the collector orchestrator. The orchestrator handles
/// all subsequent operations including database connections, task
/// management, and graceful shutdown.
///
/// ## Design Rationale
///
/// **Minimal Entry Point**: Keeps the main function simple and focused
/// on setup, delegating all complex logic to the orchestrator.
///
/// **Shared Configuration**: Uses Arc to share the CLI configuration
/// across all components without cloning.
///
/// **Async Runtime**: Uses tokio::main to set up the async runtime
/// that powers the entire collector system.
///
/// **Error Propagation**: Uses anyhow for ergonomic error handling
/// that propagates from the orchestrator up to the executable level.
///
/// # Returns
/// An error if the collector fails to initialize or run
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging for operational visibility
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()
        .ok();

    // Parse command-line arguments into shared configuration
    let cli = Arc::new(Cli::parse());

    // Create and run the collector orchestrator
    let orchestrator = CollectorOrchestrator::new(cli);
    orchestrator.run().await
}
