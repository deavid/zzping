//! ZZPing Database Application
//!
//! Server that accepts mTLS connections from collectors, stores ping data,
//! and distributes configuration updates.

use anyhow::{Context, Result};

use actix_rt::System;
use tracing_subscriber::EnvFilter;
use zzping_database::{cli::CliArgs, config::DatabaseConfig, service::DatabaseService};

fn main() -> Result<()> {
    // Use actix_rt System to install a Tokio runtime compatible with Actix.
    System::new().block_on(async_main())
}

async fn async_main() -> Result<()> {
    use clap::Parser as _;
    // Install crypto provider early
    let _ =
        rustls::crypto::CryptoProvider::install_default(rustls::crypto::ring::default_provider());
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

    // Run the service and log any returned error. If startup succeeds, block
    // forever so the process stays alive (the test harness will kill it when
    // appropriate). This matches how typical servers run until signalled.
    if let Err(e) = service.run().await {
        tracing::error!("DatabaseService.run() returned error: {:?}", e);
        return Err(anyhow::anyhow!("Database service failed: {:?}", e));
    }

    tracing::info!(
        "Database service started successfully; entering run-loop (blocking until shutdown)"
    );

    // Block until the process is killed by the test harness or user. Use a
    // oneshot receiver that is never completed to avoid adding a new dependency.
    let (_tx, rx) = tokio::sync::oneshot::channel::<()>();
    let _ = rx.await;

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
