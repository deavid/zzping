#[allow(unused_imports)]
use crate::config::{CollectorConfig, ComponentConfig, TlsConfig};
use crate::error::{CollectorError, Result};

use actix::{Actor, Addr};

// Component imports
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::builder::IntentConfigBuilder;
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

/// Builders for all components (before wiring)
struct ComponentBuilders {
    intent_config: IntentConfigBuilder<IntentConfigPermission>,
    pinger: PingerBuilder,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
}

/// Started components (running actors)
#[allow(dead_code)]
struct StartedComponents {
    intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
    pinger: PingerHandle,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
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
        // Step 2: Start ConnectionManager and network wiring
        tracing::info!("Starting ConnectionManager actor and network wiring");

        let cm_addr = self.start_connection_manager();

        // Load TLS configuration for transport layer
        tracing::info!("Loading TLS configuration for transport layer");
        let tls_cfg = Some(Self::convert_tls_config(&self.config.tls).map_err(|e| {
            CollectorError::Service(format!("Failed to convert TLS config: {}", e))
        })?);

        let addr = format!(
            "{}:{}",
            self.config.database_host, self.config.database_port
        );
        let mut network = crate::network::CollectorNetwork::new(&addr, tls_cfg, cm_addr)
            .map_err(|e| CollectorError::Service(format!("Failed to create network: {}", e)))?;

        tracing::info!("Collector service connecting to database via ConnectionManager");

        network
            .connect()
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

    fn create_builders(&self) -> Result<ComponentBuilders> {
        // Components no longer create SessionManager here; ConnectionManager will manage sessions
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

    async fn start_components(builders: ComponentBuilders) -> Result<StartedComponents> {
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
    pub fn load_tls_config(tls: &TlsConfig) -> Result<Arc<ClientConfig>> {
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
    pub fn convert_tls_config(tls: &TlsConfig) -> Result<zznet_transport_tcp::config::TlsConfig> {
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
    ) -> Result<
        zznet_hello::connection_manager::ConnectionManager<
            zzintent_config::network_messages::IntentConfigNetworkMsg,
            zzintent_config::permissions::IntentConfigPermission,
        >,
    > {
        use zznet_session::types::RoomId;

        // Collector offers intent-config related rooms
        let offered_rooms = vec![RoomId::from("intent-config")];

        Ok(zznet_hello::connection_manager::ConnectionManager::new(
            offered_rooms,
        ))
    }

    fn start_connection_manager(
        &self,
    ) -> actix::Addr<
        zznet_hello::connection_manager::ConnectionManager<
            zzintent_config::network_messages::IntentConfigNetworkMsg,
            zzintent_config::permissions::IntentConfigPermission,
        >,
    > {
        use actix::prelude::*;

        let mgr = match self.create_connection_manager() {
            Ok(m) => m,
            Err(e) => panic!("Failed to create ConnectionManager: {:?}", e),
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
            tls: TlsConfig {
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
            },
            components: ComponentConfig {
                heartbeat_interval_secs: 5,
                memdb_batch_size: 50,
            },
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
        let tls_config = TlsConfig {
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
        let tls_config = TlsConfig {
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

        use actix::Actor;

        // Create a temporary service to get ConnectionManager
        let config = create_test_config();
        let service = CollectorService::new(config).unwrap();
        let cm = service.create_connection_manager().unwrap();
        let cm_addr = cm.start();

        // Test network creation without TLS
        let network_result =
            crate::network::CollectorNetwork::new("127.0.0.1:8443", None, cm_addr.clone());
        assert!(network_result.is_ok(), "Network creation should succeed");

        // Test network creation with TLS
        let tls_config = CollectorService::convert_tls_config(&create_test_config().tls).unwrap();
        let network_result_with_tls =
            crate::network::CollectorNetwork::new("127.0.0.1:8443", Some(tls_config), cm_addr);
        assert!(
            network_result_with_tls.is_ok(),
            "Network creation with TLS should succeed"
        );
    }
}
