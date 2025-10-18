//! ZZPing Collector Application
//!
//! Integrates all collector-side components (zzpinger, zzmem-db, zzintent-config,
//! zzcollector-state) into a single binary that connects to the database server
//! via mTLS and performs network monitoring.

use anyhow::{Context, Result};
use tokio::task::LocalSet;
use tracing_subscriber::EnvFilter;
use zzping_collector::{cli::CliArgs, config::CollectorConfig, service::CollectorService};

fn main() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let local = LocalSet::new();
    rt.block_on(local.run_until(async_main()))
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

    tracing::info!("ZZPing Collector v{} starting", env!("CARGO_PKG_VERSION"));
    tracing::info!("Loading configuration from: {}", args.config);

    // Load and validate configuration
    let config = CollectorConfig::load(&args.config)
        .with_context(|| format!("Failed to load configuration from {}", args.config))?;

    config
        .validate()
        .context("Configuration validation failed")?;

    tracing::info!("Configuration loaded successfully");
    tracing::info!("Collector ID: {}", config.collector_id);
    tracing::info!(
        "Database: {}:{}",
        config.database_host,
        config.database_port
    );

    // Create and run the collector service
    let service = CollectorService::new(config).context("Failed to create collector service")?;

    service.run().await.context("Collector service failed")?;

    tracing::info!("ZZPing Collector shutdown complete");
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
