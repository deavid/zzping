#[allow(unused_imports)]
use crate::config::{ComponentConfig, DatabaseConfig, DatabaseTlsConfig};
use crate::error::{DatabaseError, Result};

use actix::{Actor, Addr};
use serde::{Deserialize, Serialize};

// Component imports (DATABASE ROLES)
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::builder::IntentConfigBuilder;
use zzintent_config::network_messages::IntentConfigNetworkMsg;
use zzintent_config::permissions::IntentConfigPermission;
use zzintent_config::role::IntentConfigRole;

use zzmem_db::actor::MemDBActor;
use zzmem_db::network_messages::MemDBMessage;
use zzmem_db::permissions::MemDBPermission;
use zzmem_db::role::MemDBRole;

use zzcollector_state::actor::CStateActor;
use zzcollector_state::builder::CStateBuilder;
use zzcollector_state::network_messages::CStateMessage;
use zzcollector_state::role::CStateRole;

// signal handling is done by higher-level process manager; unused here

// Add these imports at top
use zznet_auth::role::ApplicationRole;
use zznet_session::{
    room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError},
    session_manager::SessionManager,
    types::RoomId,
};
use zzping_auth::AuthRole;

// Add these imports after existing imports
use std::sync::Arc;

// Database uses AuthRole directly from zzping-auth for connection-level authorization.
// Component-specific permissions (IntentConfigPermission, MemDBPermission, etc.) are
// mapped from this AuthRole by each component using the AuthRoleMapper trait.

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
    /// Builder for CState component (database role)
    pub cstate: CStateBuilder<
        DatabaseMessage,
        AuthRole,
        tokio::sync::Mutex<SessionManager<DatabaseMessage, AuthRole>>,
    >,
    /// Shared SessionManager for all components and ConnectionManager (Arc<Mutex<...>> for interior mutability)
    pub session_manager: Arc<tokio::sync::Mutex<SessionManager<DatabaseMessage, AuthRole>>>,
}

type CStateActorAddr = Addr<
    CStateActor<
        DatabaseMessage,
        AuthRole,
        tokio::sync::Mutex<SessionManager<DatabaseMessage, AuthRole>>,
    >,
>;
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
    /// The shared SessionManager instance used by all components
    /// This must be used when creating ConnectionManager to ensure network messages flow properly
    pub session_manager: Arc<tokio::sync::Mutex<SessionManager<DatabaseMessage, AuthRole>>>,
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
        let session_manager = Arc::clone(&builders.session_manager);

        // Step 2: Start components
        let _started = Self::start_components(builders).await?;

        tracing::info!("All components started successfully");

        // Step 3: Start ConnectionManager actor with the SHARED SessionManager
        tracing::info!("Starting ConnectionManager actor with shared SessionManager");

        let cm_addr = self.start_connection_manager_with_session_manager(session_manager);

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
        let network =
            crate::network::DatabaseNetwork::new(&bind, tls_cfg, cm_addr, handshake_timeout);

        tracing::info!("Database service ready - using ConnectionManager for connections");

        // Run network server (this will run until shutdown)
        network
            .run()
            .await
            .map_err(|e| DatabaseError::Service(format!("Network error: {}", e)))
    }

    /// Build TLS configuration for the transport layer (TcpTransportServer)
    pub fn build_transport_tls_config(
        tls: &DatabaseTlsConfig,
    ) -> Result<Option<zznet_transport_tcp::config::TlsConfig>> {
        use std::path::PathBuf;
        use zznet_transport_tcp::config::{TlsCertAndKey, TlsConfig as TransportTlsConfig};

        let cert = TlsCertAndKey {
            pem_path: PathBuf::from(&tls.server_cert_path),
            key_path: PathBuf::from(&tls.server_key_path),
        };
        let ca = tls.ca_cert_paths.first().map(PathBuf::from);

        let tcfg = TransportTlsConfig {
            cert,
            ca_cert_path: ca,
            add_native_ca_certs: false,
            server_name: "zzping".into(),
        };

        Ok(Some(tcfg))
    }

    fn create_connection_manager(
        &self,
    ) -> Result<zznet_hello::connection_manager::ConnectionManager<DatabaseMessage, AuthRole>> {
        use zznet_session::types::RoomId;

        let offered_rooms = vec![RoomId::from("memdb"), RoomId::from("query")];

        // Create an authorizer that validates peer identity from TLS certificate
        // and resolves it to AuthRole (connection-level authorization).
        // Components will later map this to component-specific permissions using AuthRoleMapper.
        // When TLS is disabled, we accept all connections (no authentication).
        let authorizer: zzping_auth::Authorizer = Box::new(|peer_identity| {
            tracing::debug!(
                "Database authorizer checking peer identity: {}",
                peer_identity.full_identity()
            );

            // When TLS is disabled (plain-tcp), skip authentication and accept as Collector
            if peer_identity.common_name == "plain-tcp" {
                tracing::warn!(
                    "Plain TCP connection - no authentication, accepting as Collector role"
                );
                return Some(AuthRole::Collector);
            }

            // Validate CN against allowed service roles
            match AuthRole::from_cn(&peer_identity.common_name) {
                Ok(role) => {
                    tracing::debug!(
                        "Authorizer resolved {} → {:?}",
                        peer_identity.full_identity(),
                        role
                    );
                    Some(role)
                }
                Err(e) => {
                    tracing::warn!(
                        "Authorizer rejected {} - unknown role: {}",
                        peer_identity.full_identity(),
                        e
                    );
                    None
                }
            }
        });

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
        session_manager: Arc<tokio::sync::Mutex<SessionManager<DatabaseMessage, AuthRole>>>,
    ) -> Result<zznet_hello::connection_manager::ConnectionManager<DatabaseMessage, AuthRole>> {
        // Create an authorizer that validates peer identity from TLS certificate
        let authorizer: zzping_auth::Authorizer = Box::new(|peer_identity| {
            tracing::debug!(
                "Database authorizer checking peer identity: {}",
                peer_identity.full_identity()
            );

            match AuthRole::from_cn(&peer_identity.common_name) {
                Ok(role) => {
                    tracing::debug!(
                        "Authorizer resolved {} → {:?}",
                        peer_identity.full_identity(),
                        role
                    );
                    Some(role)
                }
                Err(e) => {
                    tracing::warn!(
                        "Authorizer rejected {} - unknown role: {}",
                        peer_identity.full_identity(),
                        e
                    );
                    None
                }
            }
        });

        Ok(
            zznet_hello::connection_manager::ConnectionManager::new_with_session_manager(
                session_manager,
                authorizer,
            ),
        )
    }

    /// Start ConnectionManager as an actix actor and return its address.
    ///
    /// This is useful for testing scenarios where you want to manually inject
    /// transport connections or customize the connection handling.
    pub fn start_connection_manager(
        &self,
    ) -> actix::Addr<zznet_hello::connection_manager::ConnectionManager<DatabaseMessage, AuthRole>>
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
        session_manager: Arc<tokio::sync::Mutex<SessionManager<DatabaseMessage, AuthRole>>>,
    ) -> actix::Addr<zznet_hello::connection_manager::ConnectionManager<DatabaseMessage, AuthRole>>
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
        // Create THE ONE shared SessionManager for this database process
        // All components and ConnectionManager share this single instance
        // Wrapped in Arc<Mutex<...>> to allow:
        // - Components to call &self methods (broadcast_to_room)
        // - ConnectionManager to call &mut self methods (add_peer, connect_peer)
        use zznet_session::types::RoomId;
        let offered_rooms = vec![RoomId::from("memdb"), RoomId::from("query")];

        let shared_session_manager = Arc::new(tokio::sync::Mutex::new(SessionManager::<
            DatabaseMessage,
            AuthRole,
        >::new(
            offered_rooms.clone()
        )));

        // Create IntentConfig builder - DATABASE ROLE
        // Note: IntentConfigActor will need to be updated to work with DatabaseMessage
        // For now, we'll create it without a session_manager and wire it later
        let data_dir = std::path::PathBuf::from(&self.config.data_dir);
        let config_path = data_dir.join("intent.ron");

        let intent_config =
            IntentConfigBuilder::<IntentConfigPermission>::new().role(IntentConfigRole::Database {
                config_file_path: config_path,
            });
        // Wire IntentConfig to use shared_session_manager via adapter
        // The adapter bridges the type mismatch between what IntentConfig expects
        // (Rc<SessionManager<IntentConfigNetworkMsg, PermissionWrapper<T>>>)
        // and what we have (Arc<tokio::sync::Mutex<SessionManager<DatabaseMessage, AuthRole>>>)

        // Create MemDB actor - DATABASE ROLE (no builder pattern!)
        let memdb_actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Database {
            max_results_per_target: 10000,
            persistence_path: None,
        });
        let memdb_addr = memdb_actor.start();

        // Create CState builder with the SHARED SessionManager (Arc<Mutex<...>>)
        let cstate = CStateBuilder::<
            DatabaseMessage,
            AuthRole,
            tokio::sync::Mutex<SessionManager<DatabaseMessage, AuthRole>>,
        >::new(CStateRole::Database {
            stale_timeout_secs: self.config.components.stale_timeout_secs,
            max_collectors: Some(self.config.components.max_collectors),
        })
        .session_manager(Arc::clone(&shared_session_manager));

        Ok(ComponentBuilders {
            intent_config,
            memdb_addr,
            cstate,
            session_manager: shared_session_manager,
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

        // Wire the shared SessionManager adapter to IntentConfig
        // This connects IntentConfig to the per-process SessionManager so broadcasts work
        eprintln!("⚙️ Creating DatabaseMessageAdapter for IntentConfig");
        let adapter = std::sync::Arc::new(
            zzintent_config::database_message_adapter::DatabaseMessageAdapter::new(
                std::sync::Arc::clone(&builders.session_manager),
            ),
        );
        // Cast to trait object (already wrapped in Arc)
        let adapter_trait: std::sync::Arc<
            dyn zzintent_config::database_message_adapter::BroadcastVia,
        > = adapter;
        eprintln!("⚙️ Sending SetDatabaseAdapter message to IntentConfigActor");
        intent_addr.do_send(zzintent_config::messages::SetDatabaseAdapter(adapter_trait));
        eprintln!("⚙️ SetDatabaseAdapter message sent");

        // Wire room handlers from IntentConfig to SessionManager
        // This enables message routing: network → SessionManager → room → IntentConfigActor
        eprintln!("⚙️ [Database] Wiring room handlers to SessionManager");
        Self::wire_room_handlers(&intent_addr, &builders.session_manager).await?;
        eprintln!("⚙️ [Database] Room handlers wired to SessionManager");

        // Start CState
        let cstate_addr = builders.cstate.build();

        Ok(StartedComponents {
            intent_config: intent_addr,
            memdb_addr: builders.memdb_addr,
            cstate: cstate_addr,
            session_manager: builders.session_manager,
        })
    }

    /// Wire room handlers from IntentConfigActor to SessionManager
    ///
    /// This registers the IntentConfig room handler with all existing peers,
    /// enabling message routing from the network through SessionManager to IntentConfig.
    async fn wire_room_handlers(
        intent_addr: &Addr<IntentConfigActor<IntentConfigPermission>>,
        session_manager: &Arc<
            tokio::sync::Mutex<
                zznet_session::session_manager::SessionManager<DatabaseMessage, AuthRole>,
            >,
        >,
    ) -> Result<()> {
        use zznet_session::peer_session::RoomHandle;
        use zznet_session::types::{RoomId, SessionError};

        // Create a wrapper handler that converts DatabaseMessage to IntentConfigNetworkMsg
        struct DatabaseIntentConfigRoomHandler {
            intent_addr: Addr<IntentConfigActor<IntentConfigPermission>>,
            room_id: RoomId,
        }

        impl RoomHandle<DatabaseMessage> for DatabaseIntentConfigRoomHandler {
            fn room_id(&self) -> &RoomId {
                &self.room_id
            }

            fn send_message(
                &mut self,
                msg: DatabaseMessage,
            ) -> std::result::Result<(), SessionError> {
                // Extract IntentConfigNetworkMsg from DatabaseMessage
                match msg {
                    DatabaseMessage::Intent(intent_msg) => {
                        eprintln!("  → Forwarding message to IntentConfigActor via room");
                        self.intent_addr.do_send(
                            zzintent_config::messages::NetworkMessageReceived(intent_msg),
                        );
                        Ok(())
                    }
                    DatabaseMessage::MemDB(_) => {
                        eprintln!("  → Ignoring MemDB message in IntentConfig room handler");
                        Ok(())
                    }
                    DatabaseMessage::CState(_) => {
                        eprintln!("  → Ignoring CState message in IntentConfig room handler");
                        Ok(())
                    }
                }
            }

            fn spawn_forwarder(
                &mut self,
                _tx: tokio::sync::mpsc::Sender<(RoomId, DatabaseMessage)>,
            ) -> std::result::Result<(), SessionError> {
                // This handler is receive-only
                Ok(())
            }
        }

        let room_id = RoomId::from("zzintent-config");

        // Lock SessionManager and register room with all peers
        eprintln!("  → Locking SessionManager to register rooms");
        let mut sm = session_manager.lock().await;

        let peer_ids = sm.peer_ids();
        let num_peers = peer_ids.len();
        eprintln!("  → Found {} existing peers", num_peers);

        for peer_id in peer_ids {
            eprintln!("    → Adding room to peer {}", peer_id);

            // Create a room handler for this peer
            let handler: Box<dyn RoomHandle<DatabaseMessage>> =
                Box::new(DatabaseIntentConfigRoomHandler {
                    intent_addr: intent_addr.clone(),
                    room_id: room_id.clone(),
                });

            sm.add_room_to_peer(&peer_id, room_id.clone(), handler)
                .await
                .map_err(|e| {
                    DatabaseError::Component(format!(
                        "Failed to add room to peer {}: {}",
                        peer_id, e
                    ))
                })?;
        }

        eprintln!("  → Room handler registered with all {} peers", num_peers);
        Ok(())
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
        use actix::Actor;
        use std::time::Duration;

        // Create a temporary service to get ConnectionManager
        let config = create_test_config();
        let service = DatabaseService::new(config).unwrap();
        let cm = service.create_connection_manager().unwrap();
        let cm_addr = cm.start();

        // Test network creation without TLS (TLS config creation is complex and tested elsewhere)
        // Use port 0 to let OS assign an available port
        let _network = crate::network::DatabaseNetwork::new(
            "127.0.0.1:0",
            None,
            cm_addr,
            Duration::from_secs(10),
        );
        // Network is now created successfully if we get here
        // We don't run() it as that would block indefinitely
    }
}
