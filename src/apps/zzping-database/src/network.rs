use crate::service::StartedComponents;
use actix::Actor; // For .start() method
use std::time::Duration;
use zznet_api::transport::TransportServer;
use zznet_auth::ApplicationRole; // For from_cn() method
use zznet_hello::actor::HelloConfig;
use zznet_hello::connection_manager::{ConnectionManager, HandleTransport};
use zznet_transport_tcp::config::TlsConfig;
use zznet_transport_tcp::server::TcpTransportServer;
use zzping_auth::AuthRole;

/// DatabaseNetwork manages server-side connections following the vision architecture.
///
/// This implementation:
/// - Uses TcpTransportServer directly (no ServerBuilder)
/// - Creates ConnectionManager with shared SessionManager
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
    /// 2. Create ConnectionManager with SessionManager
    /// 3. Accept loop: spawn HelloActor for each connection
    /// 4. HelloActor handles HELLO handshake
    /// 5. Messages flow via Room<T> channels (auto-registered by components)
    ///
    /// # Arguments
    /// * `components` - Started component actors (contains SessionManager)
    pub async fn run(&self, components: &StartedComponents) -> Result<(), String> {
        tracing::info!("Starting database network on {}", self.bind_addr);

        // Step 1: Create TCP transport server
        let mut server = TcpTransportServer::new(&self.bind_addr, self.tls_config.clone())
            .await
            .map_err(|e| format!("Failed to create TCP server: {:?}", e))?;

        tracing::info!("TCP server listening on {}", self.bind_addr);

        // Step 2: Create authorizer for HELLO authentication
        let authorizer = create_database_authorizer();

        // Step 3: Get SessionManager from components
        let session_manager = components.session_manager.clone();

        // Step 4: Create ConnectionManager (manages HelloActors)
        let connection_manager =
            ConnectionManager::new_with_session_manager(session_manager.clone(), authorizer);

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
                    // 3. Register peer with SessionManager
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

/// Create the authorizer function for database connections
///
/// This validates HELLO role against TLS certificate and maps to AuthRole.
fn create_database_authorizer()
-> Box<dyn Fn(&zznet_api::types::AuthContext) -> Option<zznet_api::types::Role> + Send + Sync> {
    Box::new(|auth_ctx: &zznet_api::types::AuthContext| {
        tracing::debug!(
            "Database authorizer checking HELLO role: {}",
            auth_ctx.hello_role_str
        );

        // Validate HELLO role against TLS if TLS is present
        if let Some(ref peer_identity) = auth_ctx.peer_identity {
            tracing::debug!(
                "TLS identity present: {}, validating against HELLO role",
                peer_identity.full_identity()
            );

            // Basic validation: ensure CN matches hello_role_str
            if peer_identity.common_name != auth_ctx.hello_role_str {
                tracing::error!(
                    "TLS CN mismatch: HELLO claimed '{}' but cert CN is '{}'",
                    auth_ctx.hello_role_str,
                    peer_identity.common_name
                );
                return None;
            }
        } else {
            tracing::warn!(
                "Plain TCP connection - no TLS authentication, relying on HELLO role only"
            );
        }

        // Map HELLO role string to AuthRole, then to canonical Role
        match AuthRole::from_cn(&auth_ctx.hello_role_str) {
            Ok(role) => {
                tracing::info!("Authorized connection with role: {:?}", role);
                Some(zznet_api::types::Role::new(role.as_str()))
            }
            Err(e) => {
                tracing::warn!(
                    "Authorizer rejected HELLO role '{}' - unknown role: {}",
                    auth_ctx.hello_role_str,
                    e
                );
                None
            }
        }
    })
}
