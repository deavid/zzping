//! ZZPing Database Application
//!
//! Server that accepts mTLS connections from collectors, stores ping data,
//! and distributes configuration updates.

use anyhow::{Context, Result};
use clap::Parser;
use tokio::task::LocalSet;
use tracing_subscriber::EnvFilter;
use zzping_database::{CliArgs, DatabaseConfig, DatabaseService};

fn main() -> Result<()> {
    // CRITICAL: Use LocalSet for Actix compatibility (spawn_local support)
    let rt = tokio::runtime::Runtime::new()?;
    let local = LocalSet::new();
    local.block_on(&rt, async_main())
}

async fn async_main() -> Result<()> {
    // Parse command-line arguments
    let args = CliArgs::parse();

    // Initialize logging based on CLI flags
    init_logging(&args);

    tracing::info!("ZZPing Database v{} starting", env!("CARGO_PKG_VERSION"));
    tracing::info!("Loading configuration from: {}", args.config);

    // Load and validate configuration
    let config = DatabaseConfig::load(&args.config)
        .with_context(|| format!("Failed to load configuration from {}", args.config))?;

    config
        .validate()
        .context("Configuration validation failed")?;

    tracing::info!("Configuration loaded successfully");
    tracing::info!("Binding to {}:{}", config.bind_host, config.bind_port);

    // Create and run the database service
    let service = DatabaseService::new(config).context("Failed to create database service")?;

    service.run().await.context("Database service failed")?;

    tracing::info!("ZZPing Database shutdown complete");
    Ok(())
}

/// Initialize logging based on CLI arguments.
fn init_logging(args: &CliArgs) {
    let filter = if args.trace {
        EnvFilter::new("trace")
    } else if args.debug {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .init();
}
