use crate::config::DatabaseConfig;
use crate::error::{DatabaseError, Result};

use actix::{Actor, Addr};
use serde::{Deserialize, Serialize};

// Component imports (DATABASE ROLES)
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::builder::IntentConfigBuilder;
use zzintent_config::network_messages::IntentConfigMessage;
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

use tokio::signal::unix::{signal, SignalKind};

// Add these imports at top
use rustls::server::AllowAnyAuthenticatedClient;
use rustls::{Certificate, PrivateKey, RootCertStore, ServerConfig};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use zznet_auth::{error::AuthError, role::ApplicationRole};
use zznet_session::{
    room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError},
    session_manager::SessionManager,
    types::RoomId,
};

// Add these imports after existing imports
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

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
    Intent(IntentConfigMessage),
    MemDB(MemDBMessage),
    CState(CStateMessage),
}

impl From<IntentConfigMessage> for DatabaseMessage {
    fn from(msg: IntentConfigMessage) -> Self {
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
        if let Ok(msg) = IntentConfigMessage::deserialize_for_room(room_id, bytes) {
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
        rooms.extend(IntentConfigMessage::supported_rooms());
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

/// Started components (running actors)
/// These addresses are cloned for each connection handler
#[derive(Clone)]
struct StartedComponents {
    intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    cstate: Addr<
        CStateActor<DatabaseMessage, DatabaseRole, SessionManager<DatabaseMessage, DatabaseRole>>,
    >,
}

/// Per-connection handler for collector connections
struct ConnectionHandler {
    peer_addr: SocketAddr,
    peer_role: DatabaseRole,
    stream: TlsStream<TcpStream>,
    components: StartedComponents,
}

impl ConnectionHandler {
    /// Create new connection handler
    fn new(
        peer_addr: SocketAddr,
        peer_role: DatabaseRole,
        stream: TlsStream<TcpStream>,
        components: StartedComponents,
    ) -> Self {
        Self {
            peer_addr,
            peer_role,
            stream,
            components,
        }
    }

    /// Run the connection message loop
    async fn run(mut self) -> Result<()> {
        tracing::info!(
            "Connection handler started for {} (role: {:?})",
            self.peer_addr,
            self.peer_role
        );

        let mut buffer = vec![0u8; 8192]; // 8KB buffer

        loop {
            // Read message length (4 bytes, big-endian)
            let mut len_bytes = [0u8; 4];
            match self.stream.read_exact(&mut len_bytes).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    tracing::info!("Client {} disconnected", self.peer_addr);
                    break;
                }
                Err(e) => {
                    tracing::error!("Failed to read message length from {}: {}", self.peer_addr, e);
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

            // Read message body
            match self.stream.read_exact(&mut buffer[..msg_len]).await {
                Ok(_) => {
                    tracing::debug!("Received {} bytes from {}", msg_len, self.peer_addr);

                    // Deserialize and handle message
                    if let Err(e) = self.handle_message(&buffer[..msg_len]).await {
                        tracing::error!("Failed to handle message from {}: {}", self.peer_addr, e);
                        // Continue processing other messages
                    }
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

    /// Handle a single received message
    async fn handle_message(&self, data: &[u8]) -> Result<()> {
        // Try to deserialize as DatabaseMessage
        let msg: DatabaseMessage = ron::de::from_bytes(data)
            .map_err(|e| DatabaseError::Service(format!("Failed to deserialize message: {}", e)))?;

        self.route_message(msg).await
    }

    /// Route incoming message to appropriate component
    async fn route_message(&self, msg: DatabaseMessage) -> Result<()> {
        tracing::debug!("Routing message: {:?}", msg);

        match msg {
            DatabaseMessage::Intent(intent_msg) => {
                tracing::debug!("Routing to IntentConfig: {:?}", intent_msg);
                // self.components.intent_config.send(intent_msg).await
                //     .map_err(|e| DatabaseError::Component(format!("IntentConfig send failed: {}", e)))?;
                // For now, just log
                tracing::info!("Would send to IntentConfig component");
            }
            DatabaseMessage::MemDB(memdb_msg) => {
                tracing::debug!("Routing to MemDB: {:?}", memdb_msg);
                // self.components.memdb_addr.send(memdb_msg).await
                //     .map_err(|e| DatabaseError::Component(format!("MemDB send failed: {}", e)))?;
                // For now, just log
                tracing::info!("Would send to MemDB component");
            }
            DatabaseMessage::CState(cstate_msg) => {
                tracing::debug!("Routing to CState: {:?}", cstate_msg);
                // self.components.cstate.send(cstate_msg).await
                //     .map_err(|e| DatabaseError::Component(format!("CState send failed: {}", e)))?;
                // For now, just log
                tracing::info!("Would send to CState component");
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

                            // Spawn connection handler (non-blocking)
                            tokio::spawn(async move {
                                if let Err(e) = Self::handle_connection(stream, peer_addr, acceptor, started).await {
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
        let intent_config =
            IntentConfigBuilder::<IntentConfigPermission>::new().role(IntentConfigRole::Database {
                config_file_path: "intent.ron".into(),
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
        // 1. Load CA certificate (to verify client certificates from collectors)
        let ca_file = File::open(&tls.ca_cert_path)
            .map_err(|e| DatabaseError::Config(format!("Failed to open CA file: {}", e)))?;
        let mut ca_reader = BufReader::new(ca_file);
        let ca_certs: Vec<Certificate> = certs(&mut ca_reader)
            .map_err(|e| DatabaseError::Config(format!("Failed to parse CA certs: {}", e)))?
            .into_iter()
            .map(Certificate)
            .collect();

        if ca_certs.is_empty() {
            return Err(DatabaseError::Config("No CA certificates found".into()));
        }

        let mut root_store = RootCertStore::empty();
        for cert in ca_certs {
            root_store
                .add(&cert)
                .map_err(|e| DatabaseError::Config(format!("Failed to add CA cert: {}", e)))?;
        }

        // 2. Load server certificate
        let cert_file = File::open(&tls.server_cert_path)
            .map_err(|e| DatabaseError::Config(format!("Failed to open server cert: {}", e)))?;
        let mut cert_reader = BufReader::new(cert_file);
        let cert_chain: Vec<Certificate> = certs(&mut cert_reader)
            .map_err(|e| DatabaseError::Config(format!("Failed to parse server cert: {}", e)))?
            .into_iter()
            .map(Certificate)
            .collect();

        if cert_chain.is_empty() {
            return Err(DatabaseError::Config("No server certificate found".into()));
        }

        // 3. Load server private key
        let key_file = File::open(&tls.server_key_path)
            .map_err(|e| DatabaseError::Config(format!("Failed to open server key: {}", e)))?;
        let mut key_reader = BufReader::new(key_file);
        let mut keys: Vec<PrivateKey> = pkcs8_private_keys(&mut key_reader)
            .map_err(|e| DatabaseError::Config(format!("Failed to parse private key: {}", e)))?
            .into_iter()
            .map(PrivateKey)
            .collect();

        if keys.is_empty() {
            return Err(DatabaseError::Config("No private key found".into()));
        }
        let private_key = keys.remove(0);

        // 4. Build server config (NOT client config!)
        let client_verifier = AllowAnyAuthenticatedClient::new(root_store);

        let config = ServerConfig::builder()
            .with_safe_defaults()
            .with_client_cert_verifier(Arc::new(client_verifier))
            .with_single_cert(cert_chain, private_key)
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
        let handler = ConnectionHandler::new(peer_addr, peer_role, tls_stream, components);
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
