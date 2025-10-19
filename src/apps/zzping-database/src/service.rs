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

use tokio::signal::unix::{SignalKind, signal};

// Add these imports at top
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zznet_auth::{error::AuthError, role::ApplicationRole};
use zznet_hello::protocol::{Frame, HandshakeFrame};
use zznet_session::{
    room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError},
    session_manager::SessionManager,
    types::RoomId,
};

// Add these imports after existing imports
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

// Application roles for database
#[derive(Debug, Clone, PartialEq, Eq, Copy, Serialize, Deserialize)]
pub enum DatabaseRole {
    Database,
    Collector,
    Admin,
}

impl ApplicationRole for DatabaseRole {
    fn as_str(&self) -> &'static str {
        match self {
            DatabaseRole::Database => "database",
            DatabaseRole::Collector => "collector",
            DatabaseRole::Admin => "admin",
        }
    }

    fn from_cn(cn: &str) -> std::result::Result<Self, AuthError> {
        // Extract role from CN (format: "role-name" or "name-role")
        let cn_lower = cn.to_lowercase();

        if cn_lower.contains("database") {
            Ok(DatabaseRole::Database)
        } else if cn_lower.contains("collector") {
            Ok(DatabaseRole::Collector)
        } else if cn_lower.contains("admin") {
            Ok(DatabaseRole::Admin)
        } else {
            Err(AuthError::UnknownRole(cn.to_string()))
        }
    }

    fn can_connect_to(&self, other: &Self) -> bool {
        match (self, other) {
            // Collectors connect to database
            (DatabaseRole::Collector, DatabaseRole::Database) => true,
            // Database accepts collectors
            (DatabaseRole::Database, DatabaseRole::Collector) => true,
            // Admin can connect to anything
            (DatabaseRole::Admin, _) => true,
            (_, DatabaseRole::Admin) => true,
            // Same role can connect (testing)
            (a, b) if a == b => true,
            _ => false,
        }
    }

    fn can_access_room(&self, room_id: &str) -> bool {
        match self {
            DatabaseRole::Admin => true,    // Admin has full access
            DatabaseRole::Database => true, // Database has full access
            DatabaseRole::Collector => {
                // Collectors can access their own rooms
                room_id.starts_with("collector_")
                    || room_id.starts_with("ping_")
                    || room_id.starts_with("config_")
            }
        }
    }
}

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
struct ComponentBuilders {
    intent_config: IntentConfigBuilder<IntentConfigPermission>,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    cstate:
        CStateBuilder<DatabaseMessage, DatabaseRole, SessionManager<DatabaseMessage, DatabaseRole>>,
}

/// Started components (running actors).
///
/// These addresses are cloned for each connection handler and will be used
/// in Phase 6 to route messages to components via Actix messaging.
#[derive(Clone)]
struct StartedComponents {
    intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    cstate: Addr<
        CStateActor<DatabaseMessage, DatabaseRole, SessionManager<DatabaseMessage, DatabaseRole>>,
    >,
}

/// Per-connection handler for collector connections
/// Per-connection handler for collector connections
struct ConnectionHandler {
    peer_addr: SocketAddr,
    peer_role: DatabaseRole,
    stream: TlsStream<TcpStream>,
    /// Component addresses for message routing (will be actively used in Phase 6)
    components: StartedComponents,
    /// Timeout for reading a single message frame (in milliseconds)
    message_frame_timeout_ms: u64,
}

impl ConnectionHandler {
    /// Create new connection handler
    fn new(
        peer_addr: SocketAddr,
        peer_role: DatabaseRole,
        stream: TlsStream<TcpStream>,
        components: StartedComponents,
        message_frame_timeout_ms: u64,
    ) -> Self {
        Self {
            peer_addr,
            peer_role,
            stream,
            components,
            message_frame_timeout_ms,
        }
    }

    /// Run the connection message loop
    async fn run(mut self) -> Result<()> {
        tracing::info!(
            "Connection handler started for {} (role: {:?}) - timeout: {}ms",
            self.peer_addr,
            self.peer_role,
            self.message_frame_timeout_ms
        );

        // Perform HELLO handshake before entering message loop
        if let Err(e) = self.perform_hello_handshake().await {
            tracing::error!("HELLO handshake failed for {}: {}", self.peer_addr, e);
            return Err(e);
        }

        let mut buffer = vec![0u8; 8192]; // 8KB buffer
        let timeout = if self.message_frame_timeout_ms > 0 {
            Some(std::time::Duration::from_millis(
                self.message_frame_timeout_ms,
            ))
        } else {
            None
        };

        loop {
            // Read message length (4 bytes, big-endian) with timeout
            let mut len_bytes = [0u8; 4];
            match self.read_with_timeout(&mut len_bytes, timeout).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    tracing::info!("Client {} disconnected", self.peer_addr);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    tracing::error!(
                        "Message frame timeout while reading length from {} - closing connection",
                        self.peer_addr
                    );
                    return Err(DatabaseError::MessageFrameTimeout(format!(
                        "Timeout waiting for message length from {}",
                        self.peer_addr
                    )));
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to read message length from {}: {}",
                        self.peer_addr,
                        e
                    );
                    break;
                }
            }

            let msg_len = u32::from_be_bytes(len_bytes) as usize;

            if msg_len == 0 {
                tracing::warn!("Received zero-length message from {}", self.peer_addr);
                continue;
            }

            if msg_len > buffer.len() {
                tracing::debug!("Resizing buffer from {} to {} bytes", buffer.len(), msg_len);
                buffer.resize(msg_len, 0);
            }

            // Read message body with timeout
            match self
                .read_with_timeout(&mut buffer[..msg_len], timeout)
                .await
            {
                Ok(_) => {
                    tracing::debug!("Received {} bytes from {}", msg_len, self.peer_addr);

                    // Deserialize and handle message
                    if let Err(e) = self.handle_message(&buffer[..msg_len]).await {
                        tracing::error!("Failed to handle message from {}: {}", self.peer_addr, e);
                        // Continue processing other messages
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    tracing::error!(
                        "Message frame timeout while reading {} bytes from {} - closing connection",
                        msg_len,
                        self.peer_addr
                    );
                    return Err(DatabaseError::MessageFrameTimeout(format!(
                        "Timeout waiting for message body from {}",
                        self.peer_addr
                    )));
                }
                Err(e) => {
                    tracing::error!("Failed to read message body from {}: {}", self.peer_addr, e);
                    break;
                }
            }
        }

        tracing::info!("Connection handler stopping for {}", self.peer_addr);
        Ok(())
    }

    /// Perform HELLO handshake protocol
    async fn perform_hello_handshake(&mut self) -> Result<()> {
        tracing::info!("Starting HELLO handshake with {}", self.peer_addr);

        // Database offers "memdb" and "query" rooms
        let offered_rooms = vec!["memdb".to_string(), "query".to_string()];
        tracing::info!(
            "Database offering {} rooms for connection: {:?}",
            offered_rooms.len(),
            offered_rooms
        );

        // 1. Send HELLO frame
        let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
            version: "1.0".to_string(),
            role_str: "database".to_string(),
            hostname: "database".to_string(), // TODO: get actual hostname
        });
        self.send_frame(&hello_frame).await?;
        tracing::debug!("Sent HELLO frame to {}", self.peer_addr);

        // 2. Receive HELLO frame from peer
        let _peer_hello = self.receive_frame().await?;
        match _peer_hello {
            Frame::Handshake(HandshakeFrame::Hello {
                version,
                role_str,
                hostname,
            }) => {
                tracing::info!(
                    "Received HELLO from {}: version={}, role={}, hostname={}",
                    self.peer_addr,
                    version,
                    role_str,
                    hostname
                );
            }
            _ => {
                return Err(DatabaseError::Service(format!(
                    "Expected HELLO frame, got {:?}",
                    _peer_hello
                )));
            }
        };

        // 3. Send OFFER frame
        let offer_frame = Frame::Handshake(HandshakeFrame::Offer {
            rooms: offered_rooms.clone(),
        });
        self.send_frame(&offer_frame).await?;
        tracing::debug!(
            "Sent OFFER frame with rooms {:?} to {}",
            offered_rooms,
            self.peer_addr
        );

        // 4. Receive OFFER frame from peer
        let peer_offer = self.receive_frame().await?;
        let peer_offered_rooms = match peer_offer {
            Frame::Handshake(HandshakeFrame::Offer { rooms }) => {
                tracing::info!(
                    "Received OFFER from {} with rooms {:?}",
                    self.peer_addr,
                    rooms
                );
                rooms
            }
            _ => {
                return Err(DatabaseError::Service(format!(
                    "Expected OFFER frame, got {:?}",
                    peer_offer
                )));
            }
        };

        // 5. Calculate intersection of rooms (both sides must agree)
        let mut selected_rooms = Vec::new();
        for room in &offered_rooms {
            if peer_offered_rooms.contains(room) {
                selected_rooms.push(room.clone());
            }
        }

        // 6. Send ACK frame
        let ack_frame = Frame::Handshake(HandshakeFrame::Ack {
            rooms: selected_rooms.clone(),
        });
        self.send_frame(&ack_frame).await?;
        tracing::debug!(
            "Sent ACK frame with rooms {:?} to {}",
            selected_rooms,
            self.peer_addr
        );

        // 7. Receive ACK frame from peer
        let peer_ack = self.receive_frame().await?;
        let peer_selected_rooms = match peer_ack {
            Frame::Handshake(HandshakeFrame::Ack { rooms }) => {
                tracing::info!(
                    "Received ACK from {} with rooms {:?}",
                    self.peer_addr,
                    rooms
                );
                rooms
            }
            _ => {
                return Err(DatabaseError::Service(format!(
                    "Expected ACK frame, got {:?}",
                    peer_ack
                )));
            }
        };

        // Verify both sides agreed on the same rooms
        if selected_rooms != peer_selected_rooms {
            return Err(DatabaseError::Service(format!(
                "Room negotiation failed: local={:?}, peer={:?}",
                selected_rooms, peer_selected_rooms
            )));
        }

        tracing::info!(
            "HELLO handshake completed successfully with {} - negotiated rooms: {:?}",
            self.peer_addr,
            selected_rooms
        );
        Ok(())
    }

    /// Send a HELLO frame over the connection
    async fn send_frame(&mut self, frame: &Frame) -> Result<()> {
        let data = frame
            .serialize()
            .map_err(|e| DatabaseError::Service(format!("Failed to serialize frame: {}", e)))?;

        // Send length prefix (4 bytes, big-endian)
        let len_bytes = (data.len() as u32).to_be_bytes();
        self.stream
            .write_all(&len_bytes)
            .await
            .map_err(|e| DatabaseError::Service(format!("Failed to send frame length: {}", e)))?;

        // Send frame data
        self.stream
            .write_all(&data)
            .await
            .map_err(|e| DatabaseError::Service(format!("Failed to send frame data: {}", e)))?;

        Ok(())
    }

    /// Receive a HELLO frame from the connection
    async fn receive_frame(&mut self) -> Result<Frame> {
        // Read length prefix (4 bytes, big-endian)
        let mut len_bytes = [0u8; 4];
        self.stream
            .read_exact(&mut len_bytes)
            .await
            .map_err(|e| DatabaseError::Service(format!("Failed to read frame length: {}", e)))?;

        let frame_len = u32::from_be_bytes(len_bytes) as usize;
        if frame_len == 0 {
            return Err(DatabaseError::Service(
                "Received zero-length frame".to_string(),
            ));
        }

        // Read frame data
        let mut frame_data = vec![0u8; frame_len];
        self.stream
            .read_exact(&mut frame_data)
            .await
            .map_err(|e| DatabaseError::Service(format!("Failed to read frame data: {}", e)))?;

        // Deserialize frame
        Frame::deserialize(&frame_data)
            .map_err(|e| DatabaseError::Service(format!("Failed to deserialize frame: {}", e)))
    }

    /// Read with optional timeout
    async fn read_with_timeout(
        &mut self,
        buf: &mut [u8],
        timeout: Option<std::time::Duration>,
    ) -> std::io::Result<()> {
        match timeout {
            Some(dur) => match tokio::time::timeout(dur, self.stream.read_exact(buf)).await {
                Ok(result) => result.map(|_| ()),
                Err(_) => Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Message frame read timeout",
                )),
            },
            None => self.stream.read_exact(buf).await.map(|_| ()),
        }
    }

    /// Handle a single received message
    async fn handle_message(&self, data: &[u8]) -> Result<()> {
        // Try to deserialize as DatabaseMessage
        let msg: DatabaseMessage = ron::de::from_bytes(data)
            .map_err(|e| DatabaseError::Service(format!("Failed to deserialize message: {}", e)))?;

        self.route_message(msg, &self.peer_addr.to_string()).await
    }

    /// Route incoming message to appropriate component
    async fn route_message(&self, msg: DatabaseMessage, peer_id: &str) -> Result<()> {
        tracing::debug!("Routing message: {:?}", msg);

        match msg {
            DatabaseMessage::Intent(intent_msg) => {
                tracing::debug!("Routing to IntentConfig: {:?}", intent_msg);
                self.components
                    .intent_config
                    .send(intent_msg)
                    .await
                    .map_err(|e| {
                        DatabaseError::Component(format!("IntentConfig send failed: {}", e))
                    })?;
                tracing::info!("✓ Sent to IntentConfig component");
            }
            DatabaseMessage::MemDB(memdb_msg) => {
                tracing::debug!("Routing to MemDB: {:?}", memdb_msg);
                self.components
                    .memdb_addr
                    .send(memdb_msg)
                    .await
                    .map_err(|e| DatabaseError::Component(format!("MemDB send failed: {}", e)))?;
                tracing::info!("✓ Sent to MemDB component");
            }
            DatabaseMessage::CState(cstate_msg) => {
                tracing::debug!("Routing to CState: {:?}", cstate_msg);
                // CStateActor expects WrappedCStateMessage, not raw CStateMessage
                use zzcollector_state::messages::WrappedCStateMessage;
                use zznet_session::types::PeerId;

                let wrapped_msg = WrappedCStateMessage {
                    peer_id: PeerId::from(peer_id),
                    message: cstate_msg,
                };

                self.components
                    .cstate
                    .send(wrapped_msg)
                    .await
                    .map_err(|e| DatabaseError::Component(format!("CState send failed: {}", e)))?;
                tracing::info!("✓ Sent to CState component");
            }
        }

        Ok(())
    }
}

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
        let started = Self::start_components(builders).await?;

        tracing::info!("All components started successfully");

        // Step 2: Load TLS configuration
        tracing::info!("Loading TLS configuration");
        let tls_config = Self::load_tls_config(&self.config.tls)?;
        let acceptor = TlsAcceptor::from(tls_config);
        tracing::info!("TLS acceptor ready");

        // Step 3: Create TCP listener
        let listener = self.create_listener().await?;

        // Step 4: Setup signal handlers
        let mut sigterm = signal(SignalKind::terminate())
            .map_err(|e| DatabaseError::Service(format!("Failed to setup SIGTERM: {}", e)))?;
        let mut sigint = signal(SignalKind::interrupt())
            .map_err(|e| DatabaseError::Service(format!("Failed to setup SIGINT: {}", e)))?;

        tracing::info!("Database service ready - accepting connections");

        // Step 5: Main accept loop with graceful shutdown
        loop {
            tokio::select! {
                // Accept new connection
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, peer_addr)) => {
                            tracing::info!("Accepted connection from {}", peer_addr);

                            // Clone for move into spawned task
                            let acceptor = acceptor.clone();
                            let started = started.clone();
                            let timeout_ms = self.config.components.message_frame_timeout_ms;

                            // Spawn connection handler (non-blocking)
                            tokio::spawn(async move {
                                if let Err(e) = Self::handle_connection(stream, peer_addr, acceptor, started, timeout_ms).await {
                                    tracing::error!("Connection handler error for {}: {}", peer_addr, e);
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("Failed to accept connection: {}", e);
                            // Don't break - keep accepting other connections
                        }
                    }
                }

                // Shutdown signals
                _ = sigterm.recv() => {
                    tracing::info!("Received SIGTERM, shutting down gracefully");
                    break;
                }
                _ = sigint.recv() => {
                    tracing::info!("Received SIGINT (Ctrl+C), shutting down gracefully");
                    break;
                }
            }
        }

        tracing::info!("Database service stopped");
        Ok(())
    }

    fn create_builders(&self) -> Result<ComponentBuilders> {
        // Create IntentConfig builder - DATABASE ROLE
        // Use configured data_dir (resolved by DatabaseConfig::load) to compute
        // the path for intent.ron so relative paths in RON are interpreted
        // relative to the config file location.
        // `data_dir` is mandatory and already resolved by DatabaseConfig::load()
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

        // Create CState builder - DATABASE ROLE
        let cstate = CStateBuilder::<
            DatabaseMessage,
            DatabaseRole,
            SessionManager<DatabaseMessage, DatabaseRole>,
        >::new(CStateRole::Database {
            stale_timeout_secs: self.config.components.stale_timeout_secs,
            max_collectors: Some(self.config.components.max_collectors),
        });

        Ok(ComponentBuilders {
            intent_config,
            memdb_addr,
            cstate,
        })
    }

    async fn start_components(builders: ComponentBuilders) -> Result<StartedComponents> {
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

    /// Load TLS configuration for mTLS server
    pub fn load_tls_config(tls: &crate::config::TlsConfig) -> Result<Arc<ServerConfig>> {
        // Install default crypto provider for rustls (ignore if already installed)
        let _ = rustls::crypto::CryptoProvider::install_default(
            rustls::crypto::ring::default_provider(),
        );
        // 1. Load CA certificates (to verify client certificates from collectors)
        let mut root_store = RootCertStore::empty();

        if tls.ca_cert_paths.is_empty() {
            return Err(DatabaseError::Config(
                "At least one CA certificate path required".into(),
            ));
        }

        for ca_path in &tls.ca_cert_paths {
            let ca_file = File::open(ca_path).map_err(|e| {
                DatabaseError::Config(format!("Failed to open CA file {}: {}", ca_path, e))
            })?;
            let mut ca_reader = BufReader::new(ca_file);
            let ca_certs: Vec<_> = certs(&mut ca_reader)
                .map(|r| {
                    r.map_err(|e| {
                        DatabaseError::Config(format!(
                            "Failed to parse CA certs from {}: {}",
                            ca_path, e
                        ))
                    })
                    .map(|c| Box::leak(c.as_ref().to_vec().into_boxed_slice()))
                })
                .collect::<std::result::Result<_, _>>()?;

            if ca_certs.is_empty() {
                return Err(DatabaseError::Config(format!(
                    "No CA certificates in {}",
                    ca_path
                )));
            }

            for cert in &ca_certs {
                root_store
                    .add(CertificateDer::from(&**cert))
                    .map_err(|e| DatabaseError::Config(format!("Failed to add CA cert: {}", e)))?;
            }
            tracing::info!("Loaded CA certificates from: {}", ca_path);
        }

        // 2. Load server certificate
        let cert_file = File::open(&tls.server_cert_path)
            .map_err(|e| DatabaseError::Config(format!("Failed to open server cert: {}", e)))?;
        let mut cert_reader = BufReader::new(cert_file);
        let cert_chain: Vec<_> = certs(&mut cert_reader)
            .map(|r| {
                r.map_err(|e| DatabaseError::Config(format!("Failed to parse server cert: {}", e)))
                    .map(|c| Box::leak(c.as_ref().to_vec().into_boxed_slice()))
            })
            .collect::<std::result::Result<_, _>>()?;

        if cert_chain.is_empty() {
            return Err(DatabaseError::Config("No server certificate found".into()));
        }

        // 3. Load server private key
        let key_file = File::open(&tls.server_key_path)
            .map_err(|e| DatabaseError::Config(format!("Failed to open server key: {}", e)))?;
        let mut key_reader = BufReader::new(key_file);
        let keys: Vec<_> = pkcs8_private_keys(&mut key_reader)
            .map(|r| {
                r.map_err(|e| DatabaseError::Config(format!("Failed to parse private key: {}", e)))
                    .map(|k| Box::leak(k.secret_pkcs8_der().to_vec().into_boxed_slice()))
            })
            .collect::<std::result::Result<_, _>>()?;

        if keys.is_empty() {
            return Err(DatabaseError::Config("No private key found".into()));
        }
        let private_key = PrivateKeyDer::try_from(unsafe { &*(keys[0] as *const [u8]) })
            .map_err(|e| DatabaseError::Config(format!("Invalid private key: {}", e)))?;
        drop(keys);

        // 4. Build server config (NOT client config!)
        // Use WebPkiClientVerifier builder to create a verifier that requests
        // client certificates and validates them against the provided root_store (mTLS).
        let roots = Arc::new(root_store);
        let verifier = WebPkiClientVerifier::builder(roots).build().map_err(|e| {
            DatabaseError::Config(format!("Failed to build client verifier: {}", e))
        })?;
        let cert_chain_der: Vec<_> = cert_chain
            .iter()
            .map(|c| CertificateDer::from(unsafe { &*(*c as *const [u8]) }))
            .collect();
        drop(cert_chain);

        let config = ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(cert_chain_der, private_key)
            .map_err(|e| DatabaseError::Config(format!("Failed to build TLS config: {}", e)))?;

        Ok(Arc::new(config))
    }

    /// Create TCP listener bound to configured address
    async fn create_listener(&self) -> Result<TcpListener> {
        let bind_addr = format!("{}:{}", self.config.bind_host, self.config.bind_port);

        tracing::info!("Binding TCP listener to {}", bind_addr);

        let listener = TcpListener::bind(&bind_addr).await.map_err(|e| {
            DatabaseError::Service(format!("Failed to bind to {}: {}", bind_addr, e))
        })?;

        let local_addr = listener
            .local_addr()
            .map_err(|e| DatabaseError::Service(format!("Failed to get local addr: {}", e)))?;

        tracing::info!("TCP listener bound successfully to {}", local_addr);

        Ok(listener)
    }

    /// Handle a single collector connection
    async fn handle_connection(
        stream: TcpStream,
        peer_addr: SocketAddr,
        acceptor: TlsAcceptor,
        components: StartedComponents,
        message_frame_timeout_ms: u64,
    ) -> Result<()> {
        tracing::debug!("Starting TLS handshake with {}", peer_addr);

        // Perform TLS handshake
        let tls_stream = acceptor.accept(stream).await.map_err(|e| {
            DatabaseError::Tls(format!("TLS handshake failed with {}: {}", peer_addr, e))
        })?;

        tracing::info!("TLS handshake successful with {}", peer_addr);

        // Extract client certificate and determine role
        let peer_role = Self::extract_client_role(&tls_stream, peer_addr)?;

        tracing::info!(
            "Client {} authenticated as role: {:?}",
            peer_addr,
            peer_role
        );

        // Create and run connection handler
        let handler = ConnectionHandler::new(
            peer_addr,
            peer_role,
            tls_stream,
            components,
            message_frame_timeout_ms,
        );
        handler.run().await?;

        tracing::info!("Connection closed for {}", peer_addr);
        Ok(())
    }

    /// Extract client role from certificate
    fn extract_client_role(
        tls_stream: &TlsStream<TcpStream>,
        peer_addr: SocketAddr,
    ) -> Result<DatabaseRole> {
        let (_io, session) = tls_stream.get_ref();
        let peer_certs = session.peer_certificates();

        match peer_certs {
            Some(certs) if !certs.is_empty() => {
                // For now, assume first cert and use simple CN extraction
                // TODO: Proper X.509 parsing

                // Placeholder: All authenticated clients are collectors
                tracing::debug!(
                    "Client {} presented {} certificate(s) - assuming Collector role",
                    peer_addr,
                    certs.len()
                );
                Ok(DatabaseRole::Collector)
            }
            _ => Err(DatabaseError::Tls(format!(
                "Client {} did not present certificate",
                peer_addr
            ))),
        }
    }
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
            tls: TlsConfig {
                ca_cert_paths: vec![certs_dir.join("ca.pem").to_str().unwrap().to_string()],
                server_cert_path: certs_dir.join("database.pem").to_str().unwrap().to_string(),
                server_key_path: certs_dir.join("database.key").to_str().unwrap().to_string(),
            },
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

        let result = DatabaseService::load_tls_config(&tls_config);
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
            ca_cert_paths: vec!["nonexistent.pem".into()],
            server_cert_path: certs_dir.join("database.pem").to_str().unwrap().to_string(),
            server_key_path: certs_dir.join("database.key").to_str().unwrap().to_string(),
        };

        let result = DatabaseService::load_tls_config(&tls_config);
        assert!(result.is_err(), "Should fail with missing CA");
    }
}
