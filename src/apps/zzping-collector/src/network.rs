//! Manages outgoing client connections to a remote zznet server.

use actix::Actor;
use std::time::Duration;
use zzintent_config::actor::IntentConfigActor;
use zzmem_db::actor::MemDBActor;
use zznet_api::error::TransportError;
use zznet_api::transport::TransportClient;
use zznet_hello::actor::HelloConfig;
use zznet_hello::connection_manager::{ConnectionManager, HandleTransport};
use zznet_router::RouterActor;
use zznet_transport_tcp::client::TcpTransportClient;
use zznet_transport_tcp::config::TlsConfig;
use zzpinger::scheduler::PingerSchedulerActor;

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
        allowed_roles.insert(zznet_api::types::Role::new("database"));
        allowed_roles.insert(zznet_api::types::Role::new("collector"));

        // Step 2: Components are ready (peer_manager no longer needed by ConnectionManager)

        let connection_manager = ConnectionManager::new(
            components.router_actor.clone().recipient(),
            "collector".to_string(),
            allowed_roles,
        );

        let connection_manager_addr = connection_manager.start();

        tracing::info!("ConnectionManager started, ready to connect");

        // Step 4: Reconnection loop
        loop {
            match self.try_connect(&connection_manager_addr).await {
                Ok(()) => {
                    tracing::info!("Connection established successfully");
                    // Connection succeeded, but may have closed - retry
                }
                Err(e) => {
                    tracing::error!("Connection failed: {}", e);
                }
            }

            tracing::info!("Reconnecting in {:?}...", self.reconnect_delay);
            tokio::time::sleep(self.reconnect_delay).await;
        }
    }

    /// Attempt a single connection to the database server
    async fn try_connect(
        &self,
        connection_manager_addr: &actix::Addr<ConnectionManager>,
    ) -> Result<(), NetworkError> {
        // Step 1: Connect to server using pre-validated client
        let transport = self
            .client
            .connect()
            .await
            .map_err(NetworkError::ConnectionFailed)?;

        tracing::info!("TCP connection established to {}", self.client.addr());

        // Step 2: Create HELLO configuration
        let hello_config = HelloConfig {
            hostname: "collector".to_string(),
            our_role: "collector".to_string(),
            offered_rooms: vec!["intent-config".to_string(), "memdb".to_string()],
            handshake_timeout: self.handshake_timeout,
        };

        // Step 3: Hand transport to ConnectionManager
        let handle_msg = HandleTransport {
            transport,
            config: hello_config,
        };

        let result = connection_manager_addr
            .send(handle_msg)
            .await
            .map_err(|e| {
                NetworkError::ConnectionManager(format!("Failed to send transport: {:?}", e))
            })?;

        // Check if ConnectionManager accepted the transport
        result
            .map_err(|e| NetworkError::ConnectionManager(format!("Transport rejected: {}", e)))?;

        // TODO: Wait for connection to close or error
        // For now, return immediately and let reconnection loop handle it
        Ok(())
    }
}
