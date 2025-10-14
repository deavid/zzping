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
use rustls::{Certificate, PrivateKey, RootCertStore, ServerConfig};
use rustls::server::AllowAnyAuthenticatedClient;
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use zznet_auth::{error::AuthError, role::ApplicationRole};
use zznet_session::{
    room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError},
    session_manager::SessionManager,
    types::RoomId,
};

// Placeholder types to satisfy trait bounds
#[derive(Debug, Clone, PartialEq, Eq, Copy, Serialize, Deserialize)]
pub enum DatabaseRole {
    Database,
    Admin,
}

impl ApplicationRole for DatabaseRole {
    fn as_str(&self) -> &'static str {
        match self {
            DatabaseRole::Database => "database",
            DatabaseRole::Admin => "admin",
        }
    }

    fn from_cn(_cn: &str) -> std::result::Result<Self, AuthError> {
        // For now, we'll just return an error.
        Err(AuthError::UnknownRole("Unknown".to_string()))
    }

    fn can_connect_to(&self, _other: &Self) -> bool {
        // For now, we'll allow all connections.
        true
    }

    fn can_access_room(&self, _room_id: &str) -> bool {
        // For now, we'll allow access to all rooms.
        true
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
// This struct is intentionally unused for now, but will be used in the future
// to hold the addresses of the started actors.
struct StartedComponents {
    intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    cstate:
        Addr<CStateActor<DatabaseMessage, DatabaseRole, SessionManager<DatabaseMessage, DatabaseRole>>>,
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
        let _started = Self::start_components(builders).await?;

        tracing::info!("All components started successfully");

        // TODO: TLS server setup in Day 3
        // TODO: Connection acceptance in Day 4

        // Setup signal handlers
        let mut sigterm = signal(SignalKind::terminate())
            .map_err(|e| DatabaseError::Service(format!("Failed to setup SIGTERM: {}", e)))?;
        let mut sigint = signal(SignalKind::interrupt())
            .map_err(|e| DatabaseError::Service(format!("Failed to setup SIGINT: {}", e)))?;

        tracing::info!("Database service running - press Ctrl+C to stop");

        // Main loop - wait for shutdown signal
        tokio::select! {
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM, shutting down gracefully");
            }
            _ = sigint.recv() => {
                tracing::info!("Received SIGINT (Ctrl+C), shutting down gracefully");
            }
        }

        tracing::info!("Database service stopped");
        Ok(())
    }

    fn create_builders(&self) -> Result<ComponentBuilders> {
        // Create IntentConfig builder - DATABASE ROLE
        let intent_config = IntentConfigBuilder::<IntentConfigPermission>::new().role(
            IntentConfigRole::Database {
                config_file_path: "intent.ron".into(),
            },
        );

        // Create MemDB actor - DATABASE ROLE (no builder pattern!)
        let memdb_actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Database {
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
            return Err(DatabaseError::Config(
                "No server certificate found".into(),
            ));
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
}
