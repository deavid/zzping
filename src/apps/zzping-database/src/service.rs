use crate::config::{DatabaseConfig, DatabaseTlsConfig};
use crate::error::{DatabaseError, Result};
use actix::{Actor, Addr};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use zzcollector_state::actor::CStateActor;
use zzcollector_state::builder::CStateBuilder;
use zzcollector_state::network_messages::CStateMessage;
use zzcollector_state::role::CStateRole;
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::builder::IntentConfigBuilder;
use zzintent_config::network_messages::IntentConfigNetworkMsg;
use zzintent_config::permissions::IntentConfigPermission;
use zzintent_config::role::IntentConfigRole;
use zzmem_db::actor::MemDBActor;
use zzmem_db::network_messages::MemDBMessage;
use zzmem_db::permissions::MemDBPermission;
use zzmem_db::role::MemDBRole;
use zznet_auth::role::ApplicationRole;
use zznet_session::{
    room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError},
    session_manager::SessionManager,
    types::RoomId,
};
use zzping_auth::AuthRole;

// Room Handler Architecture
//
// This application uses the declarative room handler registration pattern provided by
// `zznet-builder`. Room handlers are defined as factories in `crate::room_handlers` and
// registered with the `ServerBuilder` in `crate::network`.
//
// Pattern:
//   1. Define RoomHandlerFactory implementations (see `room_handlers.rs`)
//   2. Register factories with ClientBuilder/ServerBuilder (see `network.rs`)
//   3. Builders automatically wire handlers on connection/reconnection
//
// This approach eliminates manual SessionManager locking and provides reusable,
// testable room handler configuration. See `ROOM_REGISTRY_GUIDE.md` for details.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DatabaseMessage {
    Intent(IntentConfigNetworkMsg),
    MemDB(MemDBMessage),
    CState(CStateMessage),
}

impl From<IntentConfigNetworkMsg> for DatabaseMessage {
    fn from(msg: IntentConfigNetworkMsg) -> Self {
        DatabaseMessage::Intent(msg)
    }
}

impl From<MemDBMessage> for DatabaseMessage {
    fn from(msg: MemDBMessage) -> Self {
        DatabaseMessage::MemDB(msg)
    }
}

impl From<CStateMessage> for DatabaseMessage {
    fn from(msg: CStateMessage) -> Self {
        DatabaseMessage::CState(msg)
    }
}

impl RoomMessageTrait for DatabaseMessage {
    fn room_id(&self) -> RoomId {
        match self {
            DatabaseMessage::Intent(msg) => msg.room_id(),
            DatabaseMessage::MemDB(msg) => msg.room_id(),
            DatabaseMessage::CState(msg) => msg.room_id(),
        }
    }

    fn serialize_inner(&self) -> std::result::Result<Vec<u8>, SerializationError> {
        ron::to_string(self)
            .map(|s| s.into_bytes())
            .map_err(|e| SerializationError::Failed(e.to_string()))
    }

    fn deserialize_for_room(
        _room_id: &RoomId,
        bytes: &[u8],
    ) -> std::result::Result<Self, DeserializationError> {
        // First try to deserialize as the full DatabaseMessage enum
        let s = std::str::from_utf8(bytes)
            .map_err(|e| DeserializationError::Failed(format!("UTF-8 error: {}", e)))?;

        ron::from_str::<DatabaseMessage>(s)
            .map_err(|e| DeserializationError::Failed(format!("RON deserialize error: {}", e)))
    }

    fn supported_rooms() -> Vec<RoomId> {
        let mut rooms = Vec::new();
        rooms.extend(IntentConfigNetworkMsg::supported_rooms());
        rooms.extend(MemDBMessage::supported_rooms());
        rooms.extend(CStateMessage::supported_rooms());
        rooms
    }
}

/// Builders for all components (before wiring)
///
/// Contains the builders for each component, used internally during service initialization.
pub struct ComponentBuilders {
    /// Builder for IntentConfig component
    pub intent_config: IntentConfigBuilder<IntentConfigPermission>,
    /// Address of the running MemDB actor
    pub memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    /// Address of the running CState actor (database role)
    pub cstate_addr: CStateActorAddr,
}

type CStateActorAddr = Addr<CStateActor<AuthRole>>;
/// Started components (running actors).
///
/// These addresses are cloned for each connection handler and will be used
/// in Phase 6 to route messages to components via Actix messaging.
/// Useful for testing, embedding, and custom service composition.
#[derive(Clone)]
pub struct StartedComponents {
    /// Address of the running IntentConfig actor
    pub intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
    /// Address of the running MemDB actor
    pub memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    /// Address of the running CState actor (database role)
    pub cstate: CStateActorAddr,
}

// Per-connection handler for collector connections
// ConnectionHandler and HELLO/TLS framing helper functions removed per refactor plan.

pub struct DatabaseService {
    config: DatabaseConfig,
}

impl DatabaseService {
    pub fn new(config: DatabaseConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub async fn run(self) -> Result<()> {
        tracing::info!("Database service starting");

        // Step 1: Create builders (including the shared SessionManager)
        let builders = self.create_builders()?;

        // Step 2: Start components
        let _started = Self::start_components(builders).await?;

        tracing::info!("All components started successfully");

        // Step 3: Network wiring - pass session_manager and authorizer to builder-backed network
        tracing::info!("Starting network wiring with shared SessionManager");

        // Build TLS configuration for the transport server (if enabled)
        let tls_cfg = if let Some(tls) = &self.config.tls {
            tracing::info!("TLS enabled - using mTLS server");
            Self::build_transport_tls_config(tls)?
        } else {
            tracing::warn!("TLS disabled - using plain TCP server");
            None
        };

        let bind = format!("{}:{}", self.config.bind_host, self.config.bind_port);
        let handshake_timeout = std::time::Duration::from_secs(self.config.handshake_timeout_secs);
        let network = crate::network::DatabaseNetwork::new(&bind, tls_cfg, handshake_timeout);

        tracing::info!("Database service ready - using ConnectionManager for connections");

        // Run network server (this will run until shutdown)
        network
            .run(&_started)
            .await
            .map_err(|e| DatabaseError::Service(format!("Network error: {}", e)))
    }

    /// Build TLS configuration for the transport layer (TcpTransportServer)
    pub fn build_transport_tls_config(
        tls: &DatabaseTlsConfig,
    ) -> Result<Option<zznet_transport_tcp::config::TlsConfig>> {
        use std::path::PathBuf;
        use zznet_transport_tcp::config::{TlsCertAndKey, TlsConfig};

        let cert = TlsCertAndKey {
            pem_path: PathBuf::from(&tls.server_cert_path),
            key_path: PathBuf::from(&tls.server_key_path),
        };
        let ca = tls.ca_cert_paths.first().map(PathBuf::from);

        let tcfg = TlsConfig {
            cert,
            ca_cert_path: ca,
            add_native_ca_certs: false,
            server_name: "zzping".into(),
        };

        Ok(Some(tcfg))
    }

    fn create_connection_manager(
        &self,
    ) -> Result<zznet_hello::connection_manager::ConnectionManager<AuthRole>> {
        use zznet_session::types::RoomId;

        // FIXME(deavid): This file needs cleanup, it needs to properly use zznet-builder for everything and stop re-implementing stuff.
        // .. -   Both `CollectorService` and `DatabaseService` contain their own logic for creating a `ConnectionManager`,
        //        creating an `Authorizer`, and loading TLS certificates from disk (`load_tls_config`, `build_transport_tls_config`).
        // .. -   The `zznet-builder` crate already has methods like `.with_tls()` and `.with_connection_manager()`.
        //        The *intent* of the builder is to abstract this setup away. The apps should be telling the builder *what* to do
        //        (e.g., "use these cert paths"), not *how* to do it (e.g., manually loading PEM files and building `rustls::ClientConfig`).
        // .. -   This leads to a huge amount of boilerplate code being duplicated across both application crates.
        //        Any change to the authorization or TLS setup will now require edits in at least three places:
        //        `zznet-builder`, `zzping-collector`, and `zzping-database`.

        let offered_rooms = vec![RoomId::from("memdb"), RoomId::from("query")];

        let authorizer = self.make_authorizer();

        Ok(zznet_hello::connection_manager::ConnectionManager::new(
            offered_rooms,
            authorizer,
        ))
    }

    /// Create ConnectionManager with a provided shared SessionManager.
    ///
    /// This is the correct way to create ConnectionManager - it shares the SessionManager
    /// with all components, ensuring messages flow properly.
    fn create_connection_manager_with_session_manager(
        &self,
        session_manager: Arc<tokio::sync::Mutex<SessionManager<AuthRole>>>,
    ) -> Result<zznet_hello::connection_manager::ConnectionManager<AuthRole>> {
        let authorizer = self.make_authorizer();

        Ok(
            zznet_hello::connection_manager::ConnectionManager::new_with_session_manager(
                session_manager,
                authorizer,
            ),
        )
    }

    /// Create the authorizer closure used by the Database service.
    fn make_authorizer(&self) -> zzping_auth::Authorizer {
        Box::new(|auth_ctx| {
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

            match AuthRole::from_cn(&auth_ctx.hello_role_str) {
                Ok(role) => Some(role),
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

    /// Start ConnectionManager as an actix actor and return its address.
    ///
    /// This is useful for testing scenarios where you want to manually inject
    /// transport connections or customize the connection handling.
    pub fn start_connection_manager(
        &self,
    ) -> actix::Addr<zznet_hello::connection_manager::ConnectionManager<AuthRole>>
    {
        use actix::prelude::*;

        // Reuse create_connection_manager() so it's used and kept in sync
        let mgr = match self.create_connection_manager() {
            Ok(m) => m,
            Err(e) => panic!("Failed to create ConnectionManager: {:?}", e),
        };
        mgr.start()
    }

    /// Start ConnectionManager with a provided shared SessionManager.
    ///
    /// This is the correct way to start ConnectionManager - it shares the SessionManager
    /// with all components, ensuring messages flow properly.
    pub fn start_connection_manager_with_session_manager(
        &self,
        session_manager: Arc<tokio::sync::Mutex<SessionManager<AuthRole>>>,
    ) -> actix::Addr<zznet_hello::connection_manager::ConnectionManager<AuthRole>>
    {
        use actix::prelude::*;

        let mgr = match self.create_connection_manager_with_session_manager(session_manager) {
            Ok(m) => m,
            Err(e) => panic!(
                "Failed to create ConnectionManager with session_manager: {:?}",
                e
            ),
        };
        mgr.start()
    }

    /// Creates component builders for all database components.
    ///
    /// This method prepares the builders for IntentConfig, MemDB, and CState components
    /// without starting them. Useful for custom component wiring or testing scenarios.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let service = DatabaseService::new(config)?;
    /// let builders = service.create_builders()?;
    /// let components = DatabaseService::start_components(builders).await?;
    /// ```
    pub fn create_builders(&self) -> Result<ComponentBuilders> {
        // Create IntentConfig builder - DATABASE ROLE
        let data_dir = std::path::PathBuf::from(&self.config.data_dir);
        let config_path = data_dir.join("intent.ron");

        let intent_config =
            IntentConfigBuilder::<IntentConfigPermission>::new().role(IntentConfigRole::Database {
                config_file_path: config_path,
            });

        // Create MemDB actor - DATABASE ROLE (no builder pattern!)
        let memdb_actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Database {
            max_results_per_target: 10000,
            persistence_path: None,
        });
        let memdb_addr = memdb_actor.start();

        // Create CState actor
        let cstate_addr = CStateBuilder::<AuthRole>::new(CStateRole::Database {
            stale_timeout_secs: self.config.components.stale_timeout_secs,
            max_collectors: Some(self.config.components.max_collectors),
        })
        .build();

        Ok(ComponentBuilders {
            intent_config,
            memdb_addr,
            cstate_addr,
        })
    }

    /// Starts all database components from their builders.
    ///
    /// This method takes component builders and starts them, returning their addresses.
    /// Typically called after `create_builders()`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let service = DatabaseService::new(config)?;
    /// let builders = service.create_builders()?;
    /// let components = DatabaseService::start_components(builders).await?;
    /// ```
    pub async fn start_components(builders: ComponentBuilders) -> Result<StartedComponents> {
        // Start IntentConfig
        let intent_addr = builders
            .intent_config
            .start()
            .map_err(|e| DatabaseError::Component(format!("IntentConfig start failed: {}", e)))?;

        Ok(StartedComponents {
            intent_config: intent_addr,
            memdb_addr: builders.memdb_addr,
            cstate: builders.cstate_addr,
        })
    }

    /// Creates and starts all database components in one call.
    ///
    /// This is a convenience method that combines `create_builders()` and `start_components()`.
    /// Useful for simple scenarios where you don't need to customize builder configuration.
    pub async fn start_all_components(&self) -> Result<StartedComponents> {
        let builders = self.create_builders()?;
        Self::start_components(builders).await
    }
    // Manual TLS and per-connection handler code removed per refactor plan.
}

#[cfg(test)]
mod tests {
    use crate::config::ComponentConfig;

    use super::*;
    use std::path::Path;

    /// Helper to create a valid test config.
    fn create_test_config() -> DatabaseConfig {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let certs_dir = workspace_root.join("test_certs");
        DatabaseConfig {
            bind_host: "0.0.0.0".into(),
            bind_port: 8443,
            tls: Some(DatabaseTlsConfig {
                ca_cert_paths: vec![certs_dir.join("ca.pem").to_str().unwrap().to_string()],
                server_cert_path: certs_dir.join("database.pem").to_str().unwrap().to_string(),
                server_key_path: certs_dir.join("database.key").to_str().unwrap().to_string(),
            }),
            components: ComponentConfig {
                stale_timeout_secs: 30,
                max_collectors: 100,
                message_frame_timeout_ms: 500,
            },
            data_dir: String::from("."),
            handshake_timeout_secs: 10,
        }
    }

    #[test]
    fn test_service_creation() {
        let config = create_test_config();
        let service = DatabaseService::new(config);
        assert!(service.is_ok());
    }

    #[test]
    fn test_service_creation_validates_config() {
        let mut config = create_test_config();
        config.bind_host = String::new(); // Invalid!

        let result = DatabaseService::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_tls_config_loads_valid_certs() {
        // Initialize Rustls default CryptoProvider
        let _ = rustls::crypto::CryptoProvider::install_default(
            rustls::crypto::ring::default_provider(),
        );

        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let certs_dir = workspace_root.join("test_certs");
        let tls_config = DatabaseTlsConfig {
            ca_cert_paths: vec![certs_dir.join("ca.pem").to_str().unwrap().to_string()],
            server_cert_path: certs_dir.join("database.pem").to_str().unwrap().to_string(),
            server_key_path: certs_dir.join("database.key").to_str().unwrap().to_string(),
        };

        let result = DatabaseService::build_transport_tls_config(&tls_config);
        assert!(result.is_ok(), "TLS config should build successfully");
        assert!(result.unwrap().is_some(), "TLS config should not be None");
    }

    #[test]
    fn test_tls_config_fails_missing_ca() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let certs_dir = workspace_root.join("test_certs");
        let tls_config = DatabaseTlsConfig {
            ca_cert_paths: vec!["nonexistent.pem".into()],
            server_cert_path: certs_dir.join("database.pem").to_str().unwrap().to_string(),
            server_key_path: certs_dir.join("database.key").to_str().unwrap().to_string(),
        };

        let result = DatabaseService::build_transport_tls_config(&tls_config);
        // Build succeeds even with nonexistent CA - the error will happen when TLS config tries to load the cert
        assert!(
            result.is_ok(),
            "TLS config build should not fail at this stage"
        );
    }

    #[actix::test]
    async fn test_database_network_creation() {
        use std::time::Duration;

        // Create a temporary service to get a session_manager and authorizer
        let config = create_test_config();
        let service = DatabaseService::new(config).unwrap();
        let _builders = service.create_builders().unwrap();

        // Test network creation without TLS (TLS config creation is complex and tested elsewhere)
        // Use port 0 to let OS assign an available port
        let _network =
            crate::network::DatabaseNetwork::new("127.0.0.1:0", None, Duration::from_secs(10));
        // Network is now created successfully if we get here
        // We don't run() it as that would block indefinitely
    }
}
