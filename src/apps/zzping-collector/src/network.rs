use crate::service::StartedComponents;
use actix::Actor; // For .start() method
use std::time::Duration;
use zznet_api::transport::TransportClient;
use zznet_hello::actor::HelloConfig;
use zznet_hello::connection_manager::{ConnectionManager, HandleTransport};
use zznet_transport_tcp::client::TcpTransportClient;
use zznet_transport_tcp::config::TlsConfig;
use zzping_auth::AuthRole;

/// CollectorNetwork manages client-side connections following the vision architecture.
///
/// This implementation:
/// - Uses TcpTransportClient directly (no ClientBuilder)
/// - Creates ConnectionManager with shared SessionManager
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
    /// 2. Create ConnectionManager with SessionManager
    /// 3. Connect to server
    /// 4. Hand transport to ConnectionManager via HandleTransport message
    /// 5. ConnectionManager spawns HelloActor for HELLO handshake
    /// 6. Messages flow via Room<T> channels (auto-registered by components)
    ///
    /// Automatically reconnects on failure.
    ///
    /// # Arguments
    /// * `components` - Started component actors (contains SessionManager)
    pub async fn connect(&self, components: &StartedComponents) -> Result<(), String> {
        tracing::info!("Connecting to {}", self.remote_addr);

        // Step 1: Create authorizer for HELLO authentication
        let authorizer = create_collector_authorizer();

        // Step 2: Get SessionManager from components
        let session_manager = components.session_manager.clone();

        // Step 3: Create ConnectionManager (manages HelloActors)
        let connection_manager =
            ConnectionManager::new_with_session_manager(session_manager.clone(), authorizer);

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
        connection_manager_addr: &actix::Addr<ConnectionManager<AuthRole>>,
    ) -> Result<(), String> {
        // Step 1: Create TCP transport client
        let client = if let Some(tls_config) = &self.tls_config {
            TcpTransportClient::new(self.remote_addr.clone(), Some(tls_config.clone()))
                .map_err(|e| format!("Failed to create TLS client: {:?}", e))?
        } else {
            TcpTransportClient::plain(self.remote_addr.clone())
        };

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
        // 3. Register peer with SessionManager
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

/// Create the authorizer function for collector connections
///
/// This validates HELLO role against TLS certificate and maps to AuthRole.
fn create_collector_authorizer() -> zznet_auth::acl::GenericAuthorizer<AuthRole> {
    Box::new(|auth_ctx| {
        tracing::debug!(
            "Collector authorizer checking HELLO role: {}",
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
            tracing::debug!("No TLS - allowing plain connection");
        }

        // Map HELLO role string to AuthRole enum
        match auth_ctx.hello_role_str.as_str() {
            "database" => Some(AuthRole::Database),
            "collector" => Some(AuthRole::Collector),
            _ => {
                tracing::error!("Unknown HELLO role: {}", auth_ctx.hello_role_str);
                None
            }
        }
    })
}
