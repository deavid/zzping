//! Collector network support for the zzping collector application.
//!
//! This module provides `CollectorNetwork`, a small, vision-aligned component that
//! manages outgoing client connections to a remote zznet server. Responsibilities include:
//! - creating a TCP transport client (optionally with TLS)
//! - handing the transport to the `ConnectionManager` which spawns a `HelloActor` and
//!   performs the HELLO handshake
//! - integrating with PeerManagerActor so components can auto-register via
//!   `Room<T>` channels
//! - performing a simple automatic reconnection loop on failure
//!
//! The heavy lifting (per-connection actors, registration and routing) is performed by
//! `ConnectionManager` and the HELLO protocol actors; `CollectorNetwork` focuses on
//! establishing transports and mapping HELLO authentication into canonical `Role`s.
//! See `CollectorNetwork::connect` and `try_connect` for details.

use crate::service::StartedComponents;
use actix::Actor;
use std::time::Duration;
use zznet_api::transport::TransportClient;
use zznet_hello::actor::HelloConfig;
use zznet_hello::connection_manager::{ConnectionManager, HandleTransport};
use zznet_transport_tcp::client::TcpTransportClient;
use zznet_transport_tcp::config::TlsConfig;

/// CollectorNetwork manages client-side connections following the vision architecture.
///
/// This implementation:
/// - Uses TcpTransportClient directly (no ClientBuilder)
/// - Creates ConnectionManager with PeerManagerActor
/// - Components auto-register via Room<T> pattern
/// - NO room_handler_wirer callbacks needed!
pub struct CollectorNetwork {
    remote_addr: String,
    tls_config: Option<TlsConfig>,
    reconnect_delay: Duration,
    handshake_timeout: Duration,
}

impl CollectorNetwork {
    /// Create a new CollectorNetwork
    ///
    /// # Arguments
    /// * `addr` - the remote server address to connect to (e.g., "127.0.0.1:9001")
    /// * `tls` - optional TLS configuration
    /// * `reconnect_delay` - delay between reconnection attempts
    /// * `handshake_timeout` - timeout for the HELLO handshake protocol
    pub fn new(
        addr: &str,
        tls: Option<TlsConfig>,
        reconnect_delay: Duration,
        handshake_timeout: Duration,
    ) -> Self {
        Self {
            remote_addr: addr.to_string(),
            tls_config: tls,
            reconnect_delay,
            handshake_timeout,
        }
    }

    /// Connect to the database server
    ///
    /// This is the vision-aligned implementation:
    /// 1. Create TcpTransportClient
    /// 2. Create ConnectionManager with PeerManagerActor
    /// 3. Connect to server
    /// 4. Hand transport to ConnectionManager via HandleTransport message
    /// 5. ConnectionManager spawns HelloActor for HELLO handshake
    /// 6. Messages flow via Room<T> channels (auto-registered by components)
    ///
    /// Automatically reconnects on failure.
    ///
    /// # Arguments
    /// * `components` - Started component actors (contains PeerManagerActor)
    pub async fn connect(&self, components: &StartedComponents) -> Result<(), String> {
        tracing::info!("Connecting to {}", self.remote_addr);

        // Step 1: Build allowed roles set for HELLO authentication
        let mut allowed_roles = std::collections::HashSet::new();
        allowed_roles.insert(zznet_api::types::Role::new("database"));
        allowed_roles.insert(zznet_api::types::Role::new("collector"));

        // Step 2: Components are ready (peer_manager no longer needed by ConnectionManager)

        // FIXME(audit-blocker-2): Router.register_peer() not wired after HELLO handshake
        // - ConnectionManager.room_handler_wirer signature was changed to accept PeerChannels
        // - This allows registration of peer channels with Router after handshake completes
        // - However, no Router instance is available here (components.router doesn't exist)
        // - Required: Add router field to StartedComponents, wire registration here, handle disconnect
        // - Implementation would look like:
        //   ```
        //   let app_router = components.router.clone();
        //   let wirer = Arc::new(move |_pm, channels| {
        //       Box::pin(async move {
        //           app_router.register_peer(channels).await
        //               .map_err(|e| format!("register_peer failed: {:?}", e))
        //       })
        //   });
        //   connection_manager.with_room_handler_wirer(wirer);
        //   ```
        // - Note: Current code uses Room<T> pattern which bypasses Router, so this may not be
        //   critical for functionality, but audit identifies it as a blocker for consistency
        // - See audit doc section 4.2 for details

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
    ) -> Result<(), String> {
        // Step 1: Create TCP transport client
        let client = TcpTransportClient::new(self.remote_addr.clone(), self.tls_config.clone())
            .map_err(|e| format!("Failed to create client: {:?}", e))?;

        // Step 2: Connect to server
        let transport = client
            .connect()
            .await
            .map_err(|e| format!("Failed to connect: {:?}", e))?;

        tracing::info!("TCP connection established to {}", self.remote_addr);

        // Step 3: Create HELLO configuration
        let hello_config = HelloConfig {
            hostname: "collector".to_string(),
            our_role: "collector".to_string(),
            offered_rooms: vec!["intent-config".to_string(), "memdb".to_string()],
            handshake_timeout: self.handshake_timeout,
        };

        // Step 4: Hand transport to ConnectionManager
        // ConnectionManager will:
        // 1. Spawn HelloActor for this transport
        // 2. Run HELLO handshake
        // 3. Register peer with PeerManagerActor
        // 4. Route messages to components via Room<T>
        let handle_msg = HandleTransport {
            transport,
            config: hello_config,
        };

        let result = connection_manager_addr
            .send(handle_msg)
            .await
            .map_err(|e| format!("Failed to send transport to ConnectionManager: {:?}", e))?;

        // Check if ConnectionManager accepted the transport
        result.map_err(|e| format!("ConnectionManager rejected transport: {}", e))?;

        // TODO: Wait for connection to close or error
        // For now, return immediately and let reconnection loop handle it
        Ok(())
    }
}
