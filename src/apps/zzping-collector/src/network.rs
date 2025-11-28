//! Manages outgoing client connections to a remote zznet server.

use actix::Actor;
use std::time::Duration;
use zzintent_config::IntentConfigActor;
use zzmem_db::MemDBActor;
use zznet_api::TransportError;
use zznet_hello::ConnectionManager;
use zznet_hello::HelloConfig;
use zznet_router::RouterActor;
use zznet_transport_tcp::{ReconnectConfig, TcpTransportClient, TlsConfig};
use zzpinger::PingerSchedulerActor;

/// Started components (running actors)
pub struct StartedComponents {
    /// Address of the running IntentConfig actor.
    pub intent_config: actix::Addr<IntentConfigActor>,
    /// Address of the running Pinger scheduler actor.
    pub pinger: actix::Addr<PingerSchedulerActor>,
    /// Address of the running MemDB actor.
    pub memdb_addr: actix::Addr<MemDBActor>,
    /// RouterActor for data-plane message routing.
    pub router_actor: actix::Addr<RouterActor>,
}

/// Network initialization and operation errors.
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    /// Failed to create the TCP transport client.
    #[error("Failed to create TCP transport client: {0}")]
    TransportCreation(#[from] TransportError),

    /// Failed to establish connection to server.
    #[error("Failed to connect to server: {0}")]
    ConnectionFailed(TransportError),

    /// Error occurred in connection manager.
    #[error("Connection manager error: {0}")]
    ConnectionManager(String),
}

/// Manages client-side connections.
pub struct CollectorNetwork {
    client: TcpTransportClient,
    reconnect_delay: Duration,
    handshake_timeout: Duration,
}

impl CollectorNetwork {
    /// Creates a new network manager and validates TLS configuration.
    pub fn new(
        addr: &str,
        tls: Option<TlsConfig>,
        reconnect_delay: Duration,
        handshake_timeout: Duration,
    ) -> Result<Self, NetworkError> {
        // Create TCP transport client immediately
        // This validates TLS configuration (certs existence, etc.)
        let client = TcpTransportClient::new(addr.to_string(), tls)?;

        Ok(Self {
            client,
            reconnect_delay,
            handshake_timeout,
        })
    }

    /// Establishes connection to the database server and initiates the handshake.
    ///
    /// Automatically reconnects on failure.
    pub async fn connect(&self, components: &StartedComponents) -> Result<(), NetworkError> {
        tracing::info!("Connecting to {}", self.client.addr());

        // Build allowed roles set for HELLO authentication
        let mut allowed_roles = std::collections::HashSet::new();
        allowed_roles.insert(zznet_api::Role::new("database"));
        allowed_roles.insert(zznet_api::Role::new("collector"));

        let hello_config = HelloConfig {
            hostname: "collector".to_string(),
            our_role: "collector".to_string(),
            offered_rooms: vec!["intent-config".to_string(), "memdb".to_string()],
            handshake_timeout: self.handshake_timeout,
        };

        let connection_manager = ConnectionManager::new(
            components.router_actor.clone().recipient(),
            hello_config,
            allowed_roles,
        );

        let connection_manager_addr = connection_manager.start();

        let retry_config = ReconnectConfig {
            retry_delay: self.reconnect_delay,
        };

        tracing::info!("ConnectionManager started, calling client.maintain");

        self.client
            .clone()
            .maintain(connection_manager_addr.recipient(), retry_config);

        // The maintain method runs in the background, so we need to keep the task alive
        // For now, just sleep forever
        std::future::pending::<()>().await;
        Ok(())
    }
}
