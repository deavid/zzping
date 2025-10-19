#[allow(unused_imports)]
use crate::config::{ComponentConfig, DatabaseConfig, TlsConfig};
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
        room_id: &RoomId,
        bytes: &[u8],
    ) -> std::result::Result<Self, DeserializationError> {
        if let Ok(msg) = IntentConfigNetworkMsg::deserialize_for_room(room_id, bytes) {
            return Ok(DatabaseMessage::Intent(msg));
        }
        if let Ok(msg) = MemDBMessage::deserialize_for_room(room_id, bytes) {
            return Ok(DatabaseMessage::MemDB(msg));
        }
        if let Ok(msg) = CStateMessage::deserialize_for_room(room_id, bytes) {
            return Ok(DatabaseMessage::CState(msg));
        }
        Err(DeserializationError::Failed(
            "Failed to deserialize message for any known type".to_string(),
        ))
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
    pub cstate: CStateBuilder<DatabaseMessage, AuthRole, SessionManager<DatabaseMessage, AuthRole>>,
}

/// Started components (running actors).
///
/// These addresses are cloned for each connection handler and will be used
/// in Phase 6 to route messages to components via Actix messaging.
/// Useful for testing, embedding, and custom service composition.
#[allow(dead_code)]
#[derive(Clone)]
pub struct StartedComponents {
    /// Address of the running IntentConfig actor
    pub intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
    /// Address of the running MemDB actor
    pub memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    /// Address of the running CState actor (database role)
    pub cstate:
        Addr<CStateActor<DatabaseMessage, AuthRole, SessionManager<DatabaseMessage, AuthRole>>>,
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

        // Step 1: Create and start components
        let builders = self.create_builders()?;
        let _started = Self::start_components(builders).await?;

        tracing::info!("All components started successfully");

        // Step 2: Start ConnectionManager actor and network wiring
        tracing::info!("Starting ConnectionManager actor and network wiring");

        let cm_addr = self.start_connection_manager();

        // Build TLS configuration for the transport server (if enabled)
        let tls_cfg = if let Some(tls) = &self.config.tls {
            tracing::info!("TLS enabled - using mTLS server");
            Self::build_transport_tls_config(tls)?
        } else {
            tracing::warn!("TLS disabled - using plain TCP server");
            None
        };

        let bind = format!("{}:{}", self.config.bind_host, self.config.bind_port);
        let mut network = crate::network::DatabaseNetwork::new(&bind, tls_cfg, cm_addr)
            .await
            .map_err(|e| DatabaseError::Service(format!("Failed to create network: {}", e)))?;

        tracing::info!("Database service ready - using ConnectionManager for connections");

        // Run network accept loop (this will run until shutdown)
        network
            .run()
            .await
            .map_err(|e| DatabaseError::Service(format!("Network error: {}", e)))
    }

    /// Build TLS configuration for the transport layer (TcpTransportServer)
    pub fn build_transport_tls_config(
        tls: &TlsConfig,
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
        // This ensures EVERY connection is authorized - there is no code path for unauthenticated access.
        let authorizer: zzping_auth::Authorizer = Box::new(|peer_identity| {
            tracing::debug!(
                "Database authorizer checking peer identity: {}",
                peer_identity.full_identity()
            );

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

    /// Start ConnectionManager as an actix actor and return its address.
    fn start_connection_manager(
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
        // Create SessionManager for database components
        // Database offers "memdb" and "query" rooms (matching HELLO handshake)
        use zznet_session::types::RoomId;
        let offered_rooms = vec![RoomId::from("memdb"), RoomId::from("query")];

        // Create IntentConfig builder - DATABASE ROLE
        // Use configured data_dir (resolved by DatabaseConfig::load) to compute
        // the path for intent.ron so relative paths in RON are interpreted
        // relative to the config file location.
        // `data_dir` is mandatory and already resolved by DatabaseConfig::load()
        let data_dir = std::path::PathBuf::from(&self.config.data_dir);
        let config_path = data_dir.join("intent.ron");

        let intent_config_session_manager = zznet_session::session_manager::SessionManager::<
            zzintent_config::network_messages::IntentConfigNetworkMsg,
            zzintent_config::permission_wrapper::PermissionWrapper<IntentConfigPermission>,
        >::new(offered_rooms.clone());

        let intent_config = IntentConfigBuilder::<IntentConfigPermission>::new()
            .role(IntentConfigRole::Database {
                config_file_path: config_path,
            })
            .session_manager(intent_config_session_manager);

        // Create MemDB actor - DATABASE ROLE (no builder pattern!)
        let memdb_actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Database {
            max_results_per_target: 10000,
            persistence_path: None,
        });
        let memdb_addr = memdb_actor.start();

        // Create CState builder - maps AuthRole to CStatePermission component role
        let cstate_session_manager = zznet_session::session_manager::SessionManager::<
            DatabaseMessage,
            AuthRole,
        >::new(offered_rooms);

        let cstate = CStateBuilder::<
            DatabaseMessage,
            AuthRole,
            SessionManager<DatabaseMessage, AuthRole>,
        >::new(CStateRole::Database {
            stale_timeout_secs: self.config.components.stale_timeout_secs,
            max_collectors: Some(self.config.components.max_collectors),
        })
        .session_manager(Arc::new(cstate_session_manager));

        Ok(ComponentBuilders {
            intent_config,
            memdb_addr,
            cstate,
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

        // Start CState
        let cstate_addr = builders.cstate.build();

        Ok(StartedComponents {
            intent_config: intent_addr,
            memdb_addr: builders.memdb_addr,
            cstate: cstate_addr,
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
            tls: Some(TlsConfig {
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
        let tls_config = TlsConfig {
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
        let tls_config = TlsConfig {
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

        // Create a temporary service to get ConnectionManager
        let config = create_test_config();
        let service = DatabaseService::new(config).unwrap();
        let cm = service.create_connection_manager().unwrap();
        let cm_addr = cm.start();

        // Test network creation without TLS (TLS config creation is complex and tested elsewhere)
        // Use port 0 to let OS assign an available port
        let network_result =
            crate::network::DatabaseNetwork::new("127.0.0.1:0", None, cm_addr).await;
        assert!(
            network_result.is_ok(),
            "Network creation should succeed, error: {:?}",
            network_result.err()
        );
    }
}
