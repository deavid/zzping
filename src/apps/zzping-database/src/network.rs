use crate::service::StartedComponents;
use actix::Actor;
use std::collections::HashSet;
use std::time::Duration;
use zznet_api::transport::TransportServer;
use zznet_auth::role::ApplicationRole;
use zznet_hello::actor::HelloConfig;
use zznet_hello::connection_manager::{ConnectionManager, HandleTransport};
use zznet_transport_tcp::config::TlsConfig;
use zznet_transport_tcp::server::TcpTransportServer;
use zzping_auth::AuthRole;

/// DatabaseNetwork manages server-side connections following the vision architecture.
///
/// This implementation:
/// - Uses TcpTransportServer directly (no ServerBuilder)
/// - Creates ConnectionManager with PeerManagerActor
/// - Components auto-register via Room<T> pattern
/// - NO room_handler_wirer callbacks needed!
pub struct DatabaseNetwork {
    bind_addr: String,
    tls_config: Option<TlsConfig>,
    handshake_timeout: Duration,
}

impl DatabaseNetwork {
    /// Create a new DatabaseNetwork
    ///
    /// # Arguments
    /// * `bind_addr` - the address to bind to (e.g., "0.0.0.0:9001")
    /// * `tls` - optional TLS configuration
    /// * `handshake_timeout` - timeout for the HELLO handshake protocol
    pub fn new(bind_addr: &str, tls: Option<TlsConfig>, handshake_timeout: Duration) -> Self {
        Self {
            bind_addr: bind_addr.to_string(),
            tls_config: tls,
            handshake_timeout,
        }
    }

    /// Start the server and accept connections
    ///
    /// This is the vision-aligned implementation:
    /// 1. Create TcpTransportServer
    /// 2. Create ConnectionManager with PeerManagerActor
    /// 3. Accept loop: spawn HelloActor for each connection
    /// 4. HelloActor handles HELLO handshake
    /// 5. Messages flow via Room<T> channels (auto-registered by components)
    ///
    /// # Arguments
    /// * `components` - Started component actors (contains PeerManagerActor)
    pub async fn run(&self, components: &StartedComponents) -> Result<(), String> {
        tracing::info!("Starting database network on {}", self.bind_addr);

        // Step 1: Create TCP transport server
        let mut server = TcpTransportServer::new(&self.bind_addr, self.tls_config.clone())
            .await
            .map_err(|e| format!("Failed to create TCP server: {:?}", e))?;

        tracing::info!("TCP server listening on {}", self.bind_addr);

        // Step 2: Create allowed-roles set for HELLO authentication
        // The ConnectionManager provides a convenience constructor that
        // accepts a HashSet<zznet_api::types::Role> and builds an internal
        // authorizer which validates TLS CN (if present) and checks the
        // allowed roles. Database accepts connections from collectors and
        // clients (read-only/admin) per AuthRole::can_connect_to policy.
        let mut allowed_roles = HashSet::new();
        allowed_roles.insert(zznet_api::types::Role::new(AuthRole::Collector.as_str()));
        allowed_roles.insert(zznet_api::types::Role::new(AuthRole::ClientRo.as_str()));
        allowed_roles.insert(zznet_api::types::Role::new(AuthRole::ClientAdmin.as_str()));

        // Step 3: Components are ready (peer_manager no longer needed by ConnectionManager)

        // FIXME(audit-blocker-2): Router.register_peer() not wired after HELLO handshake
        // - ConnectionManager.room_handler_wirer signature was changed to accept PeerChannels
        // - This allows registration of peer channels with Router after handshake completes
        // - However, no Router instance is available here to register with
        // - Required: Create a shared Router in service.rs, pass it here, wire registration
        // - Implementation would look like:
        //   ```
        //   let app_router = router.clone();
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

        // Step 4: Create ConnectionManager (manages HelloActors)
        let connection_manager = ConnectionManager::new(
            components.router_actor.clone(),
            "database".to_string(),
            allowed_roles,
        );

        let connection_manager_addr = connection_manager.start();

        tracing::info!("ConnectionManager started, ready to accept connections");

        // Step 5: Accept loop - hand connections to ConnectionManager
        loop {
            match server.accept().await {
                Ok(transport) => {
                    tracing::info!("Accepted connection from {:?}", transport.peer_addr());

                    // Create HELLO configuration for this connection
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

                    // Send transport to ConnectionManager
                    // ConnectionManager will:
                    // 1. Spawn HelloActor for this transport
                    // 2. Run HELLO handshake
                    // 3. Register peer with PeerManagerActor
                    // 4. Route messages to components via Room<T>
                    let handle_msg = HandleTransport {
                        transport,
                        config: hello_config,
                    };

                    let cm_addr = connection_manager_addr.clone();
                    actix::spawn(async move {
                        if let Err(e) = cm_addr.send(handle_msg).await {
                            tracing::error!(
                                "Failed to send transport to ConnectionManager: {:?}",
                                e
                            );
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to accept connection: {:?}", e);
                    // Brief pause before retrying to avoid tight error loop
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}

// Previously there was a create_database_authorizer helper here; authorization
// is now configured by passing a HashSet<zznet_api::types::Role> into
// ConnectionManager::new_with_allowed_roles above.
