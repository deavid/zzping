#[allow(unused_imports)]
use crate::config::{CollectorConfig, CollectorTlsConfig, ComponentConfig};
use crate::error::{CollectorError, Result};

use actix::{Actor, Addr};
use zznet_auth::ApplicationRole;
use zzping_auth::AuthRole;

// Component imports
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::builder::IntentConfigBuilder;
use zzintent_config::network_messages::IntentConfigNetworkMsg;
use zzintent_config::permissions::IntentConfigPermission;
use zzintent_config::role::IntentConfigRole;

use zzpinger::api::PingerHandle;
use zzpinger::builder::PingerBuilder;

use zzmem_db::actor::MemDBActor;
use zzmem_db::permissions::MemDBPermission;
use zzmem_db::role::MemDBRole;

use tokio::signal::unix::{SignalKind, signal};

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::room_message_trait::{DeserializationError, SerializationError};
use zznet_session::types::RoomId;

// REFACTOR IN PROGRESS: Room Handler Registration
//
// The new `zznet_builder::RoomRegistry` provides a cleaner way to register room handlers.
// See `crate::room_handlers` for an example using `IntentConfigRoomHandlerFactory`.
//
// Old pattern (repetitive, inlined):
//   - Manually create wrapper structs for each component/message combo
//   - Lock SessionManager and iterate peers
//   - Call add_room_to_peer directly
//
// New pattern (via RoomRegistry):
//   1. Implement RoomHandlerFactory trait (e.g., IntentConfigRoomHandlerFactory)
//   2. Create RoomRegistry with the SessionManager
//   3. Call registry.register_room_handler(room_id, factory)
//   4. Call registry.wire_all_peers() at startup
//   5. Call registry.wire_peer(peer_id) for dynamic connections
//
// This consolidates the boilerplate and makes it easy to reuse across apps.

// FIXME(deavid): We have plenty of zznet-* crates like zznet-builder, zznet-rooms that exist to abstract room wiring.
//      We need to clean up the mess on this file, and abstract everything away in zznet-* crates.
//
// **Manual Room Handler Wiring**
//
// The service logic in both apps contains complex, nearly identical functions (`wire_room_handlers`, `register_room_for_peer`) for attaching component actors to the `SessionManager`.
//
// -   **Files:**
//     -   `src/apps/zzping-collector/src/service.rs`
//     -   `src/apps/zzping-database/src/service.rs`
// -   **The Problem:**
//     -   This logic is highly repetitive and requires creating wrapper structs (`CollectorIntentConfigRoomHandler`, `DatabaseIntentConfigRoomHandler`) just to bridge the message types.
//     -   This feels like functionality that should be part of a higher-level component framework or simplified by the `zznet-builder`. The design doc `ZZPing_Component_Framework_Architecture.md` hints at this, but the implementation isn't there, leaving the apps to do the heavy lifting.

/// Top-level message enum for the Collector service.
///
/// All network messages flow through this enum to ensure type safety
/// and proper serialization/deserialization.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CollectorMessage {
    /// IntentConfig-related messages
    Intent(IntentConfigNetworkMsg),
}

// FIXME(deavid): This enum for messages shouldn't be needed. The fact that we're manually wiring each component hints at a bigger problem and a leaky abstraction

impl From<IntentConfigNetworkMsg> for CollectorMessage {
    fn from(msg: IntentConfigNetworkMsg) -> Self {
        CollectorMessage::Intent(msg)
    }
}

impl RoomMessageTrait for CollectorMessage {
    fn room_id(&self) -> RoomId {
        match self {
            CollectorMessage::Intent(msg) => msg.room_id(),
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
        // First try to deserialize as the full CollectorMessage enum
        let s = std::str::from_utf8(bytes)
            .map_err(|e| DeserializationError::Failed(format!("UTF-8 error: {}", e)))?;

        ron::from_str::<CollectorMessage>(s)
            .map_err(|e| DeserializationError::Failed(format!("RON deserialize error: {}", e)))
    }

    fn supported_rooms() -> Vec<RoomId> {
        let mut rooms = Vec::new();
        rooms.extend(IntentConfigNetworkMsg::supported_rooms());
        rooms
    }
}

/// Builders for all components (before wiring)
///
/// Contains the builders for each component, used internally during service initialization.
pub struct ComponentBuilders {
    /// Builder for IntentConfig component
    pub intent_config: IntentConfigBuilder<IntentConfigPermission>,
    /// Builder for Pinger component
    pub pinger: PingerBuilder,
    /// Address of the running MemDB actor
    pub memdb_addr: Addr<MemDBActor<MemDBPermission>>,
}

/// Started components (running actors)
///
/// Contains all the running component actors after they have been started.
/// Useful for testing, embedding, and custom service composition.
pub struct StartedComponents {
    /// Address of the running IntentConfig actor
    pub intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
    /// Handle to the running Pinger actor
    pub pinger: PingerHandle,
    /// Address of the running MemDB actor
    pub memdb_addr: Addr<MemDBActor<MemDBPermission>>,
}

#[derive(Debug)]
/// Collector service that orchestrates all components.
pub struct CollectorService {
    config: CollectorConfig,
}

impl CollectorService {
    /// Creates a new collector service.
    pub fn new(config: CollectorConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Runs the collector service.
    pub async fn run(self) -> Result<()> {
        tracing::info!("Collector service starting");

        // Step 1: Create and start components
        let builders = self.create_builders()?;
        let _started = Self::start_components(builders).await?;

        tracing::info!("All components started successfully");
        // Step 2: Network wiring (builders will create ConnectionManager internally)
        tracing::info!("Starting network wiring");

        // Use the session_manager produced by started components and create an authorizer
        tracing::info!("Loading TLS configuration for transport layer");
        let tls_cfg = if let Some(tls) = &self.config.tls {
            tracing::info!("TLS enabled - using mTLS connection");
            Some(Self::convert_tls_config(tls).map_err(|e| {
                CollectorError::Service(format!("Failed to convert TLS config: {}", e))
            })?)
        } else {
            tracing::warn!("TLS disabled - using plain TCP connection");
            None
        };

        let addr = format!(
            "{}:{}",
            self.config.database_host, self.config.database_port
        );
        let reconnect_delay = std::time::Duration::from_millis(self.config.reconnect_delay_ms);
        let network = crate::network::CollectorNetwork::new(&addr, tls_cfg, reconnect_delay);

        tracing::info!("Collector service connecting to database via ConnectionManager");

        network
            .connect(&_started.intent_config)
            .await
            .map_err(|e| CollectorError::Service(format!("Network error: {}", e)))?;

        // Debug: ask local IntentConfigActor for its current config and log it
        // This helps verify the collector's local state at connect time.
        {
            use zzintent_config::messages::GetCurrentConfig;
            let intent_addr = _started.intent_config.clone();
            match intent_addr.send(GetCurrentConfig).await {
                Ok(cfg) => tracing::info!("IntentConfig local state at connect: {:?}", cfg),
                Err(e) => tracing::warn!("Failed to get IntentConfig state: {}", e),
            }
        }

        // Step 4: Setup signal handlers
        let mut sigterm = signal(SignalKind::terminate())
            .map_err(|e| CollectorError::Service(format!("Failed to setup SIGTERM: {}", e)))?;
        let mut sigint = signal(SignalKind::interrupt())
            .map_err(|e| CollectorError::Service(format!("Failed to setup SIGINT: {}", e)))?;

        tracing::info!("Collector service running - press Ctrl+C to stop");

        // Step 5: Main loop - wait for shutdown signal
        tokio::select! {
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM, shutting down gracefully");
            }
            _ = sigint.recv() => {
                tracing::info!("Received SIGINT (Ctrl+C), shutting down gracefully");
            }
        }

        // Step 6: Graceful shutdown
        tracing::info!("Collector service stopped");

        Ok(())
    }

    /// Creates component builders for all collector components.
    ///
    /// This method prepares the builders for IntentConfig, Pinger, and MemDB components
    /// without starting them. Useful for custom component wiring or testing scenarios.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let service = CollectorService::new(config)?;
    /// let builders = service.create_builders()?;
    /// let components = CollectorService::start_components(builders).await?;
    /// ```
    pub fn create_builders(&self) -> Result<ComponentBuilders> {
        // FIXME(deavid): This file needs cleanup, it needs to properly use zznet-builder for everything and stop re-implementing stuff.
        // .. -   Both `CollectorService` and `DatabaseService` contain their own logic for creating a `ConnectionManager`,
        //        creating an `Authorizer`, and loading TLS certificates from disk (`load_tls_config`, `build_transport_tls_config`).
        // .. -   The `zznet-builder` crate already has methods like `.with_tls()` and `.with_connection_manager()`.
        //        The *intent* of the builder is to abstract this setup away. The apps should be telling the builder *what* to do
        //        (e.g., "use these cert paths"), not *how* to do it (e.g., manually loading PEM files and building `rustls::ClientConfig`).
        // .. -   This leads to a huge amount of boilerplate code being duplicated across both application crates.
        //        Any change to the authorization or TLS setup will now require edits in at least three places:
        //        `zznet-builder`, `zzping-collector`, and `zzping-database`.

        // Create IntentConfig builder with role only
        let intent_config =
            IntentConfigBuilder::<IntentConfigPermission>::new().role(IntentConfigRole::Collector);

        // Create Pinger builder
        let pinger = PingerBuilder::new().enabled(true);

        // Create MemDB actor (no builder)
        let memdb_actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector {
            buffer_size: self.config.components.memdb_batch_size,
        });
        let memdb_addr = memdb_actor.start();

        // Wire pinger with memdb
        let pinger = pinger.memdb_addr(memdb_addr.clone());

        Ok(ComponentBuilders {
            intent_config,
            pinger,
            memdb_addr,
        })
    }

    /// Creates and starts all collector components in one call.
    ///
    /// This is a convenience method that combines `create_builders()` and `start_components()`.
    /// Useful for simple scenarios where you don't need to customize builder configuration.
    pub async fn start_all_components(&self) -> Result<StartedComponents> {
        let builders = self.create_builders()?;
        Self::start_components(builders).await
    }

    /// Starts all collector components from their builders.
    ///
    /// This method takes component builders and starts them, returning their addresses.
    /// Typically called after `create_builders()`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let service = CollectorService::new(config)?;
    /// let builders = service.create_builders()?;
    /// let components = CollectorService::start_components(builders).await?;
    /// ```
    pub async fn start_components(builders: ComponentBuilders) -> Result<StartedComponents> {
        // Start IntentConfig
        let intent_addr = builders
            .intent_config
            .start()
            .map_err(|e| CollectorError::Component(format!("IntentConfig start failed: {}", e)))?;

        // Start Pinger
        let pinger_handle = builders.pinger.start()?;

        Ok(StartedComponents {
            intent_config: intent_addr,
            pinger: pinger_handle,
            memdb_addr: builders.memdb_addr,
        })
    }

    /// Load TLS configuration for mTLS client connection
    pub fn load_tls_config(tls: &CollectorTlsConfig) -> Result<Arc<ClientConfig>> {
        // 1. Load CA certificate (to verify database server)
        let ca_file = File::open(&tls.ca_cert_path)
            .map_err(|e| CollectorError::Config(format!("Failed to open CA file: {}", e)))?;
        let mut ca_reader = BufReader::new(ca_file);
        let ca_certs: Vec<_> = certs(&mut ca_reader)
            .map(|r| {
                r.map_err(|e| CollectorError::Config(format!("Failed to parse CA certs: {}", e)))
                    .map(|c| Box::leak(c.as_ref().to_vec().into_boxed_slice()))
            })
            .collect::<std::result::Result<_, _>>()?;

        if ca_certs.is_empty() {
            return Err(CollectorError::Config("No CA certificates found".into()));
        }

        let mut root_store = RootCertStore::empty();
        for cert in &ca_certs {
            root_store
                .add(CertificateDer::from(&**cert))
                .map_err(|e| CollectorError::Config(format!("Failed to add CA cert: {}", e)))?;
        }

        // 2. Load client certificate
        let cert_file = File::open(&tls.client_cert_path)
            .map_err(|e| CollectorError::Config(format!("Failed to open client cert: {}", e)))?;
        let mut cert_reader = BufReader::new(cert_file);
        let cert_chain: Vec<_> = certs(&mut cert_reader)
            .map(|r| {
                r.map_err(|e| CollectorError::Config(format!("Failed to parse client cert: {}", e)))
                    .map(|c| Box::leak(c.as_ref().to_vec().into_boxed_slice()))
            })
            .collect::<std::result::Result<_, _>>()?;

        if cert_chain.is_empty() {
            return Err(CollectorError::Config("No client certificate found".into()));
        }

        // 3. Load client private key
        let key_file = File::open(&tls.client_key_path)
            .map_err(|e| CollectorError::Config(format!("Failed to open client key: {}", e)))?;
        let mut key_reader = BufReader::new(key_file);
        let keys: Vec<_> = pkcs8_private_keys(&mut key_reader)
            .map(|r| {
                r.map_err(|e| CollectorError::Config(format!("Failed to parse private key: {}", e)))
                    .map(|k| Box::leak(k.secret_pkcs8_der().to_vec().into_boxed_slice()))
            })
            .collect::<std::result::Result<_, _>>()?;

        if keys.is_empty() {
            return Err(CollectorError::Config("No private key found".into()));
        }
        let private_key = PrivateKeyDer::try_from(unsafe { &*(keys[0] as *const [u8]) })
            .map_err(|e| CollectorError::Config(format!("Invalid private key: {}", e)))?;
        drop(keys);

        // 4. Build client config
        let cert_chain_der: Vec<_> = cert_chain
            .iter()
            .map(|c| CertificateDer::from(unsafe { &*(*c as *const [u8]) }))
            .collect();
        drop(cert_chain);
        let config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(cert_chain_der, private_key)
            .map_err(|e| CollectorError::Config(format!("Failed to build TLS config: {}", e)))?;

        Ok(Arc::new(config))
    }

    /// Convert collector TLS config to transport layer TLS config
    pub fn convert_tls_config(
        tls: &CollectorTlsConfig,
    ) -> Result<zznet_transport_tcp::config::TlsConfig> {
        use std::path::PathBuf;
        use zznet_transport_tcp::config::{TlsCertAndKey, TlsConfig as TransportTlsConfig};

        // Convert our simple TlsConfig into the transport crate's TlsConfig
        let cert = TlsCertAndKey {
            pem_path: PathBuf::from(&tls.client_cert_path),
            key_path: PathBuf::from(&tls.client_key_path),
        };
        let ca = Some(PathBuf::from(&tls.ca_cert_path));

        Ok(TransportTlsConfig {
            cert,
            ca_cert_path: ca,
            add_native_ca_certs: false,
            server_name: "zzping".into(),
        })
    }

    // old manual HELLO/TLS/framing helpers removed - Phase 3 finalization
}

impl CollectorService {
    /// Create ConnectionManager configured with offered rooms for collector
    fn create_connection_manager(
        &self,
    ) -> Result<zznet_hello::connection_manager::ConnectionManager<CollectorMessage, AuthRole>>
    {
        use zznet_session::types::RoomId;

        // Collector offers intent-config related rooms
        let offered_rooms = vec![RoomId::from("intent-config")];

        // Create an authorizer using the service helper
        let authorizer: zzping_auth::Authorizer = self.make_authorizer();

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
        session_manager: Arc<
            tokio::sync::Mutex<
                zznet_session::session_manager::SessionManager<CollectorMessage, AuthRole>,
            >,
        >,
    ) -> Result<zznet_hello::connection_manager::ConnectionManager<CollectorMessage, AuthRole>>
    {
        let authorizer: zzping_auth::Authorizer = self.make_authorizer();

        Ok(
            zznet_hello::connection_manager::ConnectionManager::new_with_session_manager(
                session_manager,
                authorizer,
            ),
        )
    }

    /// Create the authorizer closure used by the Collector service.
    fn make_authorizer(&self) -> zzping_auth::Authorizer {
        Box::new(|peer_identity| {
            tracing::debug!(
                "Collector authorizer checking peer identity: {}",
                peer_identity.full_identity()
            );

            match AuthRole::from_cn(&peer_identity.common_name) {
                Ok(role) => {
                    tracing::debug!(
                        "Collector authorizer resolved {} → {:?}",
                        peer_identity.full_identity(),
                        role
                    );
                    Some(role)
                }
                Err(e) => {
                    tracing::warn!(
                        "Collector authorizer rejected {} - unknown role: {}",
                        peer_identity.full_identity(),
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
    ) -> actix::Addr<zznet_hello::connection_manager::ConnectionManager<CollectorMessage, AuthRole>>
    {
        use actix::prelude::*;

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
        session_manager: Arc<
            tokio::sync::Mutex<
                zznet_session::session_manager::SessionManager<CollectorMessage, AuthRole>,
            >,
        >,
    ) -> actix::Addr<zznet_hello::connection_manager::ConnectionManager<CollectorMessage, AuthRole>>
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Helper to create a valid test config.
    fn create_test_config() -> CollectorConfig {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let certs_dir = workspace_root.join("test_certs");
        CollectorConfig {
            collector_id: "test-collector".into(),
            database_host: "127.0.0.1".into(),
            database_port: 8443,
            tls: Some(CollectorTlsConfig {
                ca_cert_path: certs_dir.join("ca.pem").to_str().unwrap().to_string(),
                client_cert_path: certs_dir
                    .join("collector.pem")
                    .to_str()
                    .unwrap()
                    .to_string(),
                client_key_path: certs_dir
                    .join("collector.key")
                    .to_str()
                    .unwrap()
                    .to_string(),
            }),
            components: ComponentConfig {
                heartbeat_interval_ms: 5000,
                memdb_batch_size: 50,
            },
            reconnect_delay_ms: 5000,
        }
    }

    #[test]
    fn test_service_creation() {
        let config = create_test_config();
        let service = CollectorService::new(config);
        assert!(service.is_ok());
    }

    #[test]
    fn test_service_creation_validates_config() {
        let mut config = create_test_config();
        config.collector_id = String::new(); // Invalid!

        let result = CollectorService::new(config);
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
        let tls_config = CollectorTlsConfig {
            ca_cert_path: certs_dir.join("ca.pem").to_str().unwrap().to_string(),
            client_cert_path: certs_dir
                .join("collector.pem")
                .to_str()
                .unwrap()
                .to_string(),
            client_key_path: certs_dir
                .join("collector.key")
                .to_str()
                .unwrap()
                .to_string(),
        };

        let result = CollectorService::load_tls_config(&tls_config);
        assert!(result.is_ok(), "TLS config should load successfully");
    }

    #[test]
    fn test_convert_tls_config() {
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
        let tls_config = CollectorTlsConfig {
            ca_cert_path: certs_dir.join("ca.pem").to_str().unwrap().to_string(),
            client_cert_path: certs_dir
                .join("collector.pem")
                .to_str()
                .unwrap()
                .to_string(),
            client_key_path: certs_dir
                .join("collector.key")
                .to_str()
                .unwrap()
                .to_string(),
        };

        let result = CollectorService::convert_tls_config(&tls_config);
        assert!(result.is_ok(), "TLS config conversion should succeed");

        let transport_config = result.unwrap();
        assert_eq!(transport_config.server_name, "zzping");
        assert!(!transport_config.add_native_ca_certs);
        assert!(transport_config.ca_cert_path.is_some());
        assert_eq!(
            transport_config.cert.pem_path,
            Path::new(&tls_config.client_cert_path)
        );
        assert_eq!(
            transport_config.cert.key_path,
            Path::new(&tls_config.client_key_path)
        );
    }

    #[actix::test]
    async fn test_collector_network_creation() {
        // Initialize Rustls default CryptoProvider
        let _ = rustls::crypto::CryptoProvider::install_default(
            rustls::crypto::ring::default_provider(),
        );

        use std::time::Duration;

        // Create a temporary service to get a session_manager and authorizer
        let config = create_test_config();
        let service = CollectorService::new(config).unwrap();
        let _builders = service.create_builders().unwrap();
        // Test network creation without TLS
        let _network =
            crate::network::CollectorNetwork::new("127.0.0.1:8443", None, Duration::from_secs(5));
        // Network created successfully if we get here

        // Test network creation with TLS
        if let Some(tls) = &create_test_config().tls {
            let tls_config = CollectorService::convert_tls_config(tls).unwrap();
            let _network_with_tls = crate::network::CollectorNetwork::new(
                "127.0.0.1:8443",
                Some(tls_config),
                Duration::from_secs(5),
            );
            // Network created successfully if we get here
        }
    }
}
