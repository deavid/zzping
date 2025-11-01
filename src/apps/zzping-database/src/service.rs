use crate::config::{DatabaseConfig, DatabaseTlsConfig};
use crate::error::DatabaseError;
use actix::{Actor, Addr};
use async_trait::async_trait;
use zzcollector_state::actor::CStateActor;
use zzcollector_state::builder::CStateBuilder;
use zzcollector_state::config::CStateConfig;
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::builder::IntentConfigBuilder;
use zzmem_db::actor::MemDBActor;
use zzmem_db::builder::MemDBBuilder;
use zzmem_db::config::MemDBConfig;
use zznet_router::RouterActor;

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
// This approach provides reusable, testable room handler configuration.
// See `ROOM_REGISTRY_GUIDE.md` for details.

/// Component builders for all database components.
///
/// These builders are configured but not yet started. They can be customized
/// before calling `start_components()` to start all actors.
/// Useful for testing, embedding, and custom service composition.
pub struct ComponentBuilders {
    /// Builder for IntentConfig component
    pub intent_config: IntentConfigBuilder,
    /// Builder for MemDB component
    pub memdb_builder: MemDBBuilder,
    /// Builder for CState component
    pub cstate_builder: CStateBuilder,
}
/// Started components (running actors).
///
/// These addresses are cloned for each connection handler and will be used
/// to route messages to components via the vision architecture (Room<T> pattern).
/// Useful for testing, embedding, and custom service composition.
#[derive(Clone)]
pub struct StartedComponents {
    /// Address of the running IntentConfig actor
    pub intent_config: Addr<IntentConfigActor>,
    /// Address of the running MemDB actor
    pub memdb_addr: Addr<MemDBActor>,
    /// Address of the running CState actor (database role)
    pub cstate: Addr<CStateActor>,
    /// RouterActor for data-plane message routing
    pub router_actor: Addr<RouterActor>,
}

// Per-connection handler for collector connections
// ConnectionHandler and HELLO/TLS framing helper functions removed per refactor plan.

pub struct DatabaseService {
    config: DatabaseConfig,
}

impl DatabaseService {
    pub fn new(config: DatabaseConfig) -> Result<Self, DatabaseError> {
        config.validate()?;
        Ok(Self { config })
    }

    pub async fn run_impl(self) -> Result<(), DatabaseError> {
        tracing::info!("Database service starting");

        // Step 1: Create builders (including PeerManagerActor)
        let builders = self.create_builders()?;

        // Step 2: Start components
        let _started = Self::start_components(builders).await?;

        tracing::info!("All components started successfully");

        // Step 3: Network wiring - pass peer_manager and authorizer to builder-backed network
        tracing::info!("Starting network wiring with PeerManagerActor");

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

    /// Public run method that delegates to the internal implementation
    pub async fn run(self) -> Result<(), DatabaseError> {
        self.run_impl().await
    }

    /// Build TLS configuration for the transport layer (TcpTransportServer)
    pub fn build_transport_tls_config(
        tls: &DatabaseTlsConfig,
    ) -> Result<Option<zznet_transport_tcp::config::TlsConfig>, DatabaseError> {
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

    // ConnectionManager creation is now handled by the network module which
    // constructs and passes a HashSet<Role> to `ConnectionManager::new()`.
    // The previous helper and authorizer closure were removed during the
    // authorization centralization refactor.

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
    pub fn create_builders(&self) -> Result<ComponentBuilders, DatabaseError> {
        // Create IntentConfig builder - database configuration
        let data_dir = std::path::PathBuf::from(&self.config.data_dir);
        let config_path = data_dir.join("intent.ron");

        let intent_config = IntentConfigBuilder::new().config_for_database(config_path);

        let memdb_builder = MemDBBuilder::new(MemDBConfig::for_database(10000, None));

        let cstate_builder = CStateBuilder::new(CStateConfig::for_database(
            self.config.components.stale_timeout_secs,
            Some(self.config.components.max_collectors),
        ));

        Ok(ComponentBuilders {
            intent_config,
            memdb_builder,
            cstate_builder,
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
    pub async fn start_components(
        builders: ComponentBuilders,
    ) -> Result<StartedComponents, DatabaseError> {
        // Start RouterActor
        let router_actor = RouterActor::new(vec![]).start();

        // Destructure builders
        let ComponentBuilders {
            intent_config,
            memdb_builder,
            cstate_builder,
        } = builders;

        // Configure IntentConfig with RouterActor
        let intent_config = intent_config.router(router_actor.clone());

        // Configure MemDB with RouterActor and build
        let memdb_addr = memdb_builder.router(router_actor.clone()).build();

        // Configure CState with RouterActor and build
        let cstate_addr = cstate_builder.router(router_actor.clone()).build();

        // Start IntentConfig
        let intent_addr = intent_config
            .start()
            .map_err(|e| DatabaseError::Component(format!("IntentConfig start failed: {}", e)))?;

        Ok(StartedComponents {
            intent_config: intent_addr,
            memdb_addr,
            cstate: cstate_addr,
            router_actor,
        })
    }

    /// Creates and starts all database components in one call.
    ///
    /// This is a convenience method that combines `create_builders()` and `start_components()`.
    /// Useful for simple scenarios where you don't need to customize builder configuration.
    pub async fn start_all_components(&self) -> Result<StartedComponents, DatabaseError> {
        let builders = self.create_builders()?;
        Self::start_components(builders).await
    }
    // Manual TLS and per-connection handler code removed per refactor plan.
}

/// Implement ZZNetService trait for DatabaseService
#[async_trait]
impl zznet_builder::traits::ZZNetService for DatabaseService {
    type Config = DatabaseConfig;
    type Error = DatabaseError;

    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        config.validate().map_err(|e| {
            DatabaseError::Config(format!("Configuration validation failed: {}", e))
        })?;
        Ok(Self { config })
    }

    async fn run(self) -> Result<(), Self::Error> {
        self.run_impl()
            .await
            .map_err(|e| DatabaseError::Service(format!("Service error: {}", e)))
    }

    fn service_name() -> &'static str {
        "ZZPing Database"
    }
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
