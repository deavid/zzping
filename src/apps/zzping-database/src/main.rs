//! ZZPing Database Application
//!
//! Service architecture using Actix actors directly.

use actix::prelude::*;
use anyhow::Result;
use clap::Parser;
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use zzping_database::config::DatabaseConfig;
use zznet_transport_tcp::TcpTransportServer;
use zznet_api::{serve_connections, Role};
use std::collections::HashSet;

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
            config.components.stale_timeout_secs * 1000, // Convert seconds to milliseconds
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

    // Create TCP transport server
    let server = TcpTransportServer::new(&bind_addr, tls_cfg).await?;

    // Setup connection manager
    let mut allowed_roles = HashSet::new();
    allowed_roles.insert(Role::new("collector"));
    allowed_roles.insert(Role::new("client-ro"));
    allowed_roles.insert(Role::new("client-admin"));

    let hello_config = zznet_hello::HelloConfig {
        hostname: "database".to_string(),
        our_role: "database".to_string(),
        offered_rooms: vec![
            "intent-config".to_string(),
            "memdb".to_string(),
            "query".to_string(),
        ],
        handshake_timeout,
    };

    let connection_manager = zznet_hello::ConnectionManager::new(
        router_actor.clone().recipient(),
        hello_config,
        allowed_roles,
    );

    let connection_manager_addr = connection_manager.start();

    tracing::info!("ConnectionManager started, calling serve_connections");

    serve_connections(server, connection_manager_addr.recipient());

    tracing::info!("ZZPing Database running. Press Ctrl+C to exit.");

    match tokio::signal::ctrl_c().await {
        Ok(_) => tracing::info!("Ctrl+C received. Exiting."),
        Err(e) => tracing::error!("Error listening for signal: {}", e),
    }

    Ok(())
}
