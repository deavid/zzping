//! ZZPing Database Application
//!
//! Service architecture using Actix actors directly.

use actix::prelude::*;
use anyhow::Result;
use clap::Parser;
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use zzping_database::config::DatabaseConfig;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to database configuration file
    #[arg(short, long, default_value = "database.ron")]
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

    tracing::info!("Starting ZZPing Database...");

    let config_content = std::fs::read_to_string(config_path)?;
    let config: DatabaseConfig = ron::from_str(&config_content)?;

    let router_actor = zznet_router::RouterActor::new(vec![]).start();

    let data_dir = std::path::PathBuf::from(&config.data_dir);
    let config_path = data_dir.join("intent.ron");
    let intent_builder =
        zzintent_config::IntentConfigBuilder::new().config_for_database(config_path);
    let _intent_addr = intent_builder.router(router_actor.clone()).start()?;

    let memdb_builder =
        zzmem_db::MemDBBuilder::new(zzmem_db::MemDBConfig::for_database(10000, None));
    let _memdb_addr = memdb_builder.router(router_actor.clone()).build();

    let cstate_builder =
        zzcollector_state::CStateBuilder::new(zzcollector_state::CStateConfig::for_database(
            config.components.stale_timeout_secs,
            Some(config.components.max_collectors),
        ));
    let _cstate_addr = cstate_builder.router(router_actor.clone()).build();

    let tls_cfg = if let Some(tls) = &config.tls {
        tls.to_transport_config()?
    } else {
        None
    };

    let bind_addr = format!("{}:{}", config.bind_host, config.bind_port);
    let handshake_timeout = Duration::from_secs(config.handshake_timeout_secs);

    let network =
        zzping_database::network::DatabaseNetwork::bind(&bind_addr, tls_cfg, handshake_timeout)
            .await?;

    let router_for_network = router_actor.clone();
    tokio::spawn(async move {
        if let Err(e) = network.run(&router_for_network).await {
            tracing::error!("Database network task failed: {}", e);
        }
    });

    tracing::info!("ZZPing Database running. Press Ctrl+C to exit.");

    match tokio::signal::ctrl_c().await {
        Ok(_) => tracing::info!("Ctrl+C received. Exiting."),
        Err(e) => tracing::error!("Error listening for signal: {}", e),
    }

    Ok(())
}
