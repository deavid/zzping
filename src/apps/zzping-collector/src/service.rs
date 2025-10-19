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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zznet_hello::protocol::{Frame, HandshakeFrame};

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

        // Step 3: Connect to database and start connection handler
        tracing::info!(
            "Connecting to database at {}:{}...",
            self.config.database_host,
            self.config.database_port
        );

        let tls_stream = Self::connect_to_database(
            &self.config.database_host,
            self.config.database_port,
            tls_config,
        )
        .await?;

        tracing::info!("✅ Connected to database successfully");

        // Spawn connection handler task
        let connection_task = tokio::spawn(async move {
            if let Err(e) = Self::run_connection_handler(tls_stream).await {
                tracing::error!("Connection handler failed: {}", e);
            }
        });

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

        // Step 5: Main loop - wait for shutdown signal or connection failure
        tokio::select! {
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM, shutting down gracefully");
            }
            _ = sigint.recv() => {
                tracing::info!("Received SIGINT (Ctrl+C), shutting down gracefully");
            }
            _ = connection_task => {
                tracing::info!("Connection handler task completed");
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
            zzintent_config::network_messages::IntentConfigNetworkMsg,
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
    ) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>> {
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

        Ok(tls_stream)
    }

    /// Run the connection handler with HELLO handshake and message loop
    async fn run_connection_handler(
        stream: tokio_rustls::client::TlsStream<tokio::net::TcpStream>,
    ) -> Result<()> {
        tracing::info!("Starting collector connection handler");

        // Perform HELLO handshake
        if let Err(e) = Self::perform_hello_handshake(stream).await {
            tracing::error!("HELLO handshake failed: {}", e);
            return Err(e);
        }

        tracing::info!("HELLO handshake completed, entering message loop");

        // TODO: Implement message loop for Phase 3
        // For now, just keep the connection alive
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await; // Keep alive for 1 hour

        tracing::info!("Connection handler stopping");
        Ok(())
    }

    /// Perform HELLO handshake as the collector (initiator)
    async fn perform_hello_handshake(
        mut stream: tokio_rustls::client::TlsStream<tokio::net::TcpStream>,
    ) -> Result<()> {
        tracing::info!("Starting HELLO handshake as collector");

        // Collector offers "memdb" room
        let offered_rooms = vec!["memdb".to_string()];
        tracing::info!(
            "Collector offering {} rooms for connection: {:?}",
            offered_rooms.len(),
            offered_rooms
        );

        // 1. Send HELLO frame
        let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
            version: "1.0".to_string(),
            role_str: "collector".to_string(),
            hostname: "collector".to_string(), // TODO: get actual hostname
        });
        Self::send_frame(&mut stream, &hello_frame).await?;
        tracing::debug!("Sent HELLO frame as collector");

        // 2. Receive HELLO frame from database
        let db_hello = Self::receive_frame(&mut stream).await?;
        match db_hello {
            Frame::Handshake(HandshakeFrame::Hello {
                version,
                role_str,
                hostname,
            }) => {
                tracing::info!(
                    "Received HELLO from database: version={}, role={}, hostname={}",
                    version,
                    role_str,
                    hostname
                );
            }
            _ => {
                return Err(CollectorError::Service(format!(
                    "Expected HELLO frame, got {:?}",
                    db_hello
                )));
            }
        }

        // 3. Receive OFFER frame from database
        let db_offer = Self::receive_frame(&mut stream).await?;
        let db_offered_rooms = match db_offer {
            Frame::Handshake(HandshakeFrame::Offer { rooms }) => {
                tracing::info!("Received OFFER from database with rooms {:?}", rooms);
                rooms
            }
            _ => {
                return Err(CollectorError::Service(format!(
                    "Expected OFFER frame, got {:?}",
                    db_offer
                )));
            }
        };

        // 4. Send OFFER frame
        let offer_frame = Frame::Handshake(HandshakeFrame::Offer {
            rooms: offered_rooms.clone(),
        });
        Self::send_frame(&mut stream, &offer_frame).await?;
        tracing::debug!(
            "Sent OFFER frame with rooms {:?} as collector",
            offered_rooms
        );

        // 5. Calculate intersection of rooms
        let mut selected_rooms = Vec::new();
        for room in &offered_rooms {
            if db_offered_rooms.contains(room) {
                selected_rooms.push(room.clone());
            }
        }

        // 6. Receive ACK frame from database
        let db_ack = Self::receive_frame(&mut stream).await?;
        let db_selected_rooms = match db_ack {
            Frame::Handshake(HandshakeFrame::Ack { rooms }) => {
                tracing::info!("Received ACK from database with rooms {:?}", rooms);
                rooms
            }
            _ => {
                return Err(CollectorError::Service(format!(
                    "Expected ACK frame, got {:?}",
                    db_ack
                )));
            }
        };

        // 7. Send ACK frame
        let ack_frame = Frame::Handshake(HandshakeFrame::Ack {
            rooms: selected_rooms.clone(),
        });
        Self::send_frame(&mut stream, &ack_frame).await?;
        tracing::debug!(
            "Sent ACK frame with rooms {:?} as collector",
            selected_rooms
        );

        // Verify both sides agreed on the same rooms
        if selected_rooms != db_selected_rooms {
            return Err(CollectorError::Service(format!(
                "Room negotiation failed: local={:?}, remote={:?}",
                selected_rooms, db_selected_rooms
            )));
        }

        tracing::info!(
            "HELLO handshake completed successfully as collector - negotiated rooms: {:?}",
            selected_rooms
        );
        Ok(())
    }

    /// Send a HELLO frame over the connection
    async fn send_frame(
        stream: &mut tokio_rustls::client::TlsStream<tokio::net::TcpStream>,
        frame: &Frame,
    ) -> Result<()> {
        let data = frame
            .serialize()
            .map_err(|e| CollectorError::Service(format!("Failed to serialize frame: {}", e)))?;

        // Send length prefix (4 bytes, big-endian)
        let len_bytes = (data.len() as u32).to_be_bytes();
        stream
            .write_all(&len_bytes)
            .await
            .map_err(|e| CollectorError::Service(format!("Failed to send frame length: {}", e)))?;

        // Send frame data
        stream
            .write_all(&data)
            .await
            .map_err(|e| CollectorError::Service(format!("Failed to send frame data: {}", e)))?;

        Ok(())
    }

    /// Receive a HELLO frame from the connection
    async fn receive_frame(
        stream: &mut tokio_rustls::client::TlsStream<tokio::net::TcpStream>,
    ) -> Result<Frame> {
        // Read length prefix (4 bytes, big-endian)
        let mut len_bytes = [0u8; 4];
        stream
            .read_exact(&mut len_bytes)
            .await
            .map_err(|e| CollectorError::Service(format!("Failed to read frame length: {}", e)))?;

        let frame_len = u32::from_be_bytes(len_bytes) as usize;
        if frame_len == 0 {
            return Err(CollectorError::Service(
                "Received zero-length frame".to_string(),
            ));
        }

        // Read frame data
        let mut frame_data = vec![0u8; frame_len];
        stream
            .read_exact(&mut frame_data)
            .await
            .map_err(|e| CollectorError::Service(format!("Failed to read frame data: {}", e)))?;

        // Deserialize frame
        Frame::deserialize(&frame_data)
            .map_err(|e| CollectorError::Service(format!("Failed to deserialize frame: {}", e)))
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
