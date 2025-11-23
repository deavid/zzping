//! ZZPing Collector Application
//!
//! Service architecture using Actix actors directly.

use actix::prelude::*;
use anyhow::Result;
use clap::Parser;
use surge_ping::{Client, ConfigBuilder};
use tracing_subscriber::EnvFilter;
use zzping_collector::config::CollectorConfig;
use zzping_collector::network::StartedComponents;
use zzpinger::MockPingerClient;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to collector configuration file
    #[arg(short, long, default_value = "collector.ron")]
    config: String,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

#[actix::main]
async fn main() -> Result<()> {
    // Install crypto provider early
    let _ =
        rustls::crypto::CryptoProvider::install_default(rustls::crypto::ring::default_provider());

    let args = Args::parse();
    let config_path = &args.config;
    let debug = args.debug;

    let log_level = if debug { "debug" } else { "info" };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level)),
        )
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .init();

    tracing::info!("Starting ZZPing Collector...");

    let config_content = std::fs::read_to_string(config_path)?;
    let config: CollectorConfig = ron::from_str(&config_content)?;

    let router = zznet_router::RouterActor::new(vec![]).start();

    let intent_builder = zzintent_config::IntentConfigBuilder::new().config_for_collector();
    let intent_addr = intent_builder.router(router.clone()).start()?;

    let memdb_builder = zzmem_db::MemDBBuilder::new(zzmem_db::MemDBConfig::for_collector(
        config.components.memdb_batch_size,
    ));
    let memdb_addr = memdb_builder.router(router.clone()).build();
    let memdb_recipient: actix::Recipient<zzmem_db::StorePingResult> =
        memdb_addr.clone().recipient();

    let pinger_builder = zzpinger::PingerBuilder {
        clock: None,
        spawn_strategy: zzpinger::SpawnStrategy::NewArbiter,
    };
    let pinger_addr = match config.components.pinger_backend {
        zzping_collector::config::PingerBackend::Real => {
            let ping_config = ConfigBuilder::default().build();
            tracing::info!("Creating surge_ping client...");
            let client = Client::new(&ping_config)
                .expect("Failed to create surge_ping client - check raw socket permissions");
            pinger_builder.start(client, memdb_recipient.clone())
        }
        zzping_collector::config::PingerBackend::Mock => {
            tracing::info!("Using mock pinger client for testing");
            let client = MockPingerClient::default();
            pinger_builder.start(client, memdb_recipient.clone())
        }
    };

    let tls_cfg = if let Some(tls) = &config.tls {
        tracing::info!("TLS enabled - using mTLS connection");
        Some(tls.to_transport_config()?)
    } else {
        tracing::warn!("TLS disabled - using plain TCP connection");
        None
    };

    let addr = format!("{}:{}", config.database_host, config.database_port);
    let reconnect_delay = std::time::Duration::from_millis(config.reconnect_delay_ms);
    let handshake_timeout = std::time::Duration::from_secs(10);

    let network = zzping_collector::network::CollectorNetwork::new(
        &addr,
        tls_cfg,
        reconnect_delay,
        handshake_timeout,
    )?;

    let started_components = StartedComponents {
        intent_config: intent_addr,
        pinger: pinger_addr,
        memdb_addr,
        router_actor: router,
    };

    tokio::spawn(async move {
        if let Err(e) = network.connect(&started_components).await {
            tracing::error!("Collector network task failed: {}", e);
        }
    });

    tracing::info!("ZZPing Collector running. Press Ctrl+C to exit.");

    match tokio::signal::ctrl_c().await {
        Ok(_) => tracing::info!("Ctrl+C received. Exiting."),
        Err(e) => tracing::error!("Error listening for signal: {}", e),
    }

    Ok(())
}
