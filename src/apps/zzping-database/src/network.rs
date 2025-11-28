//! Network server wiring for the database application.
//!
//! This module provides the transport server, connection manager, and
//! HELLO handshake wiring to accept collector and client connections.
//! The implementation focuses on supporting the vision architecture
//! (Room<T> pattern) and connection lifecycle management.

use actix::Actor;
use std::collections::HashSet;
use std::time::Duration;
use zznet_api::Role;
use zznet_api::TransportError;
use zznet_hello::ConnectionManager;
use zznet_hello::HelloConfig;
use zznet_transport_tcp::TcpTransportServer;
use zznet_transport_tcp::TlsConfig;

/// Errors that can occur during network initialization and operation.
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    /// Failed to create the TCP transport server.
    #[error("Failed to create TCP transport server: {0}")]
    ServerCreation(#[from] TransportError),

    /// Error occurred in connection manager.
    #[error("Connection manager error: {0}")]
    ConnectionManager(String),
}

/// DatabaseNetwork manages server-side connections following the vision architecture.
///
/// This implementation:
/// - Uses TcpTransportServer directly (no ServerBuilder)
/// - Creates ConnectionManager with PeerManagerActor
/// - Components auto-register via Room<T> pattern
/// - NO room_handler_wirer callbacks needed!
pub struct DatabaseNetwork {
    server: TcpTransportServer,
    handshake_timeout: Duration,
}

impl DatabaseNetwork {
    /// Create a new DatabaseNetwork and bind to the address
    ///
    /// # Arguments
    /// * `bind_addr` - the address to bind to (e.g., "0.0.0.0:9001")
    /// * `tls` - optional TLS configuration
    /// * `handshake_timeout` - timeout for the HELLO handshake protocol
    pub async fn bind(
        bind_addr: &str,
        tls: Option<TlsConfig>,
        handshake_timeout: Duration,
    ) -> Result<Self, NetworkError> {
        tracing::info!("Starting database network on {}", bind_addr);

        // Step 1: Create TCP transport server (binds to port)
        let server = TcpTransportServer::new(bind_addr, tls).await?;

        tracing::info!("TCP server listening on {}", bind_addr);

        Ok(Self {
            server,
            handshake_timeout,
        })
    }

    /// Start the accept loop
    pub async fn run(
        self,
        router_actor: &actix::Addr<zznet_router::RouterActor>,
    ) -> Result<(), NetworkError> {
        let mut allowed_roles = HashSet::new();
        allowed_roles.insert(Role::new("collector"));
        allowed_roles.insert(Role::new("client-ro"));
        allowed_roles.insert(Role::new("client-admin"));

        let hello_config = HelloConfig {
            hostname: "database".to_string(),
            our_role: "database".to_string(),
            offered_rooms: vec![
                "intent-config".to_string(),
                "memdb".to_string(),
                "query".to_string(),
            ],
            handshake_timeout: self.handshake_timeout,
        };

        let connection_manager = ConnectionManager::new(
            router_actor.clone().recipient(),
            hello_config,
            allowed_roles,
        );

        let connection_manager_addr = connection_manager.start();

        tracing::info!("ConnectionManager started, calling server.serve");

        self.server.serve(connection_manager_addr.recipient());

        // The serve method runs in the background, so we need to keep the task alive
        // For now, just sleep forever
        std::future::pending::<()>().await;
        Ok(())
    }
}
