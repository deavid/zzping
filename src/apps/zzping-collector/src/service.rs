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

use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
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

        // Step 2: Load TLS configuration
        tracing::info!("Loading TLS configuration...");
        let tls_config = Self::load_tls_config(&self.config.tls)?;
        tracing::info!("TLS configuration loaded successfully");

        // Step 3: Connect to database
        tracing::info!(
            "Connecting to database at {}:{}...",
            self.config.database_host,
            self.config.database_port
        );

        let connection = Self::connect_to_database(
            &self.config.database_host,
            self.config.database_port,
            tls_config,
        )
        .await;

        match connection {
            Ok(_) => tracing::info!("✅ Connected to database successfully"),
            Err(e) => tracing::error!("TCP connection failed: {}", e),
        }

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
        // Create a SessionManager and IntentConfig builder wired to it.
        // This enables networked ConfigUpdate broadcasts/receives (mTLS+SessionManager).
        use zznet_session::types::RoomId;

        // Offered rooms: intent-config room name used by the component
        let offered_rooms = vec![RoomId::from("intent-config")];

        let session_manager = zznet_session::session_manager::SessionManager::<
            zzintent_config::network_messages::IntentConfigMessage,
            zzintent_config::permission_wrapper::PermissionWrapper<IntentConfigPermission>,
        >::new(offered_rooms);

        // Create IntentConfig builder and attach session manager
        let intent_config = IntentConfigBuilder::<IntentConfigPermission>::new()
            .role(IntentConfigRole::Collector)
            .session_manager(session_manager);

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

    /// Establish mTLS connection to database
    async fn connect_to_database(
        host: &str,
        port: u16,
        tls_config: Arc<ClientConfig>,
    ) -> Result<tokio::net::TcpStream> {
        use tokio::net::TcpStream;
        use tokio_rustls::TlsConnector;

        // Connect TCP
        let addr = format!("{}:{}", host, port);
        let tcp_stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| CollectorError::Service(format!("TCP connection failed: {}", e)))?;

        tracing::debug!("TCP connection established to {}", addr);

        // Perform TLS handshake
        let connector = TlsConnector::from(tls_config);
        let domain = ServerName::try_from(host.to_owned())
            .map_err(|e| CollectorError::Config(format!("Invalid server name: {}", e)))?;

        let tls_stream = connector
            .connect(domain, tcp_stream)
            .await
            .map_err(|e| CollectorError::Service(format!("TLS handshake failed: {}", e)))?;

        tracing::debug!("TLS handshake completed successfully");

        // For Phase 4, we just prove the connection works
        // Phase 5 will add SessionManager and message routing

        // Extract the underlying TCP stream for now
        let (tcp_stream, _tls_session) = tls_stream.into_inner();

        Ok(tcp_stream)
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
            ca_cert_path: "nonexistent.pem".into(),
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
        assert!(result.is_err(), "Should fail with missing CA");
    }
}
