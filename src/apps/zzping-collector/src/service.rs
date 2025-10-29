//! Collector service and component orchestration for the zzping collector application.
//!
//! This module defines `CollectorService`, the top-level application service that
//! configures, starts, and coordinates all collector components. Responsibilities
//! include:
//! - creating shared infrastructure (PeerManagerActor) and component builders
//! - starting component actors (IntentConfig, MemDB, Pinger) and returning
//!   `StartedComponents` for integration testing or wiring
//! - loading and converting TLS configuration for transport-layer mTLS
//! - creating and starting `ConnectionManager` instances and producing the
//!   authorizer closure used by the HELLO protocol
//! - running the main lifecycle: connect, observe local state, and handle
//!   graceful shutdown via signals (SIGINT/SIGTERM)
//!
//! The module also exposes helper methods used by tests to create builders,
//! start managers, and validate TLS/transport configuration.

use crate::config::{CollectorConfig, CollectorTlsConfig};
use crate::error::CollectorError;
use actix::{Actor, Addr};
use anyhow::Result;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use tokio::signal::unix::{SignalKind, signal};
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::builder::IntentConfigBuilder;
use zzmem_db::actor::MemDBActor;
use zzmem_db::builder::MemDBBuilder;
use zzmem_db::config::MemDBConfig;
use zznet_peer_manager::PeerManagerActor;
use zznet_router::RouterActor;
use zzpinger::api::PingerHandle;
use zzpinger::builder::PingerBuilder;

// Room Handler Architecture
//
// This application uses the declarative room handler registration pattern provided by
// `zznet-builder`. Room handlers are defined as factories in `crate::room_handlers` and
// registered with the `ClientBuilder` in `crate::network`.
//
// Pattern:
//   1. Define RoomHandlerFactory implementations (see `room_handlers.rs`)
//   2. Register factories with ClientBuilder/ServerBuilder (see `network.rs`)
//   3. Builders automatically wire handlers on connection/reconnection
//
// This approach provides reusable, testable room handler configuration.
// See `ROOM_REGISTRY_GUIDE.md` for details.

/// Builders for all components (before wiring)
///
/// Contains the builders for each component, used internally during service initialization.
pub struct ComponentBuilders {
    /// Builder for IntentConfig component
    pub intent_config: IntentConfigBuilder,
    /// Builder for Pinger component
    pub pinger: PingerBuilder,
    /// Address of the running MemDB actor
    pub memdb_addr: Addr<MemDBActor>,
    /// Shared PeerManagerActor for network communication
    pub peer_manager: Addr<PeerManagerActor>,
}

/// Started components (running actors)
///
/// Contains all the running component actors after they have been started.
/// Useful for testing, embedding, and custom service composition.
pub struct StartedComponents {
    /// Address of the running IntentConfig actor
    pub intent_config: Addr<IntentConfigActor>,
    /// Handle to the running Pinger actor
    pub pinger: PingerHandle,
    /// Address of the running MemDB actor
    pub memdb_addr: Addr<MemDBActor>,
    /// Shared PeerManagerActor for network communication
    pub peer_manager: Addr<PeerManagerActor>,
    /// RouterActor for data-plane message routing
    pub router_actor: Addr<RouterActor>,
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
        let handshake_timeout = std::time::Duration::from_secs(10); // TODO: Make configurable
        let network = crate::network::CollectorNetwork::new(
            &addr,
            tls_cfg,
            reconnect_delay,
            handshake_timeout,
        );

        tracing::info!("Collector service connecting to database via ConnectionManager");

        network
            .connect(&_started)
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
        // Create the shared PeerManagerActor once for the entire application
        let peer_manager_actor = PeerManagerActor::new(None).start();

        tracing::info!("Created PeerManagerActor for components");

        // FIXME(deavid): This file needs cleanup, it needs to properly use zznet-builder for everything and stop re-implementing stuff.
        // .. -   Both `CollectorService` and `DatabaseService` contain their own logic for creating a `ConnectionManager`,
        //        creating an `Authorizer`, and loading TLS certificates from disk (`load_tls_config`, `build_transport_tls_config`).
        // .. -   The `zznet-builder` crate already has methods like `.with_tls()` and `.with_connection_manager()`.
        //        The *intent* of the builder is to abstract this setup away. The apps should be telling the builder *what* to do
        //        (e.g., "use these cert paths"), not *how* to do it (e.g., manually loading PEM files and building `rustls::ClientConfig`).
        // .. -   This leads to a huge amount of boilerplate code being duplicated across both application crates.
        //        Any change to the authorization or TLS setup will now require edits in at least three places:
        //        `zznet-builder`, `zzping-collector`, and `zzping-database`.

        // Create IntentConfig builder configured as collector and with PeerManager
        let intent_config = IntentConfigBuilder::new()
            .config_for_collector()
            .peer_manager(peer_manager_actor.clone());
        // .router(intent_config_router); // Will be set in start_components with RouterActor

        // Create Pinger builder
        let pinger = PingerBuilder::new().enabled(true);

        // Create MemDB actor (collector configuration)
        let memdb_addr = MemDBBuilder::new(MemDBConfig::for_collector(
            self.config.components.memdb_batch_size,
        ))
        .peer_manager(peer_manager_actor.clone())
        .build();

        // Wire pinger with memdb
        let pinger = pinger.memdb_addr(memdb_addr.clone());

        Ok(ComponentBuilders {
            intent_config,
            pinger,
            memdb_addr,
            peer_manager: peer_manager_actor,
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
        // Start RouterActor
        let router_actor = RouterActor::new(vec![], None).start();

        // Start IntentConfig
        let intent_addr = builders
            .intent_config
            .router(router_actor.clone())
            .start()
            .map_err(|e| CollectorError::Component(format!("IntentConfig start failed: {}", e)))?;

        // Start Pinger
        let pinger_handle = builders.pinger.start()?;

        Ok(StartedComponents {
            intent_config: intent_addr,
            pinger: pinger_handle,
            memdb_addr: builders.memdb_addr,
            peer_manager: builders.peer_manager,
            router_actor,
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
            Err(CollectorError::Config("No CA certificates found".into()))?;
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
            Err(CollectorError::Config("No client certificate found".into()))?;
        }

        // 3. Load client private key
        let key_file = File::open(&tls.client_key_path)
            .map_err(|e| CollectorError::Config(format!("Failed to open client key: {}", e)))?;
        let mut key_reader = BufReader::new(key_file);
        let keys: Vec<_> = pkcs8_private_keys(&mut key_reader)
            .map(|r| {
                r.map_err(|e| CollectorError::Config(format!("Failed to parse private key: {}", e)))
            })
            .collect::<std::result::Result<_, _>>()?;

        if keys.is_empty() {
            Err(CollectorError::Config("No private key found".into()))?;
        }
        // Convert to PrivateKeyDer directly from the owned key
        let private_key = PrivateKeyDer::Pkcs8(keys.into_iter().next().unwrap());

        // 4. Build client config - convert cert_chain to owned CertificateDer
        let cert_chain_der: Vec<CertificateDer<'static>> = cert_chain
            .into_iter()
            .map(|c| CertificateDer::from(c.to_vec()))
            .collect();
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
}

impl CollectorService {
    // ConnectionManager creation is performed by the network layer. The
    // network module constructs the allowed-role HashSet and passes it to
    // `ConnectionManager::new()`; the previous helper was removed as part of
    // the authorization refactor.
}
#[cfg(test)]
mod tests {
    use crate::config::ComponentConfig;

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
        let _network = crate::network::CollectorNetwork::new(
            "127.0.0.1:8443",
            None,
            Duration::from_secs(5),
            Duration::from_secs(10),
        );
        // Network created successfully if we get here

        // Test network creation with TLS
        if let Some(tls) = &create_test_config().tls {
            let tls_config = CollectorService::convert_tls_config(tls).unwrap();
            let _network_with_tls = crate::network::CollectorNetwork::new(
                "127.0.0.1:8443",
                Some(tls_config),
                Duration::from_secs(5),
                Duration::from_secs(10),
            );
            // Network created successfully if we get here
        }
    }
}
