use crate::service::{DatabaseMessage, StartedComponents};
use std::time::Duration;
use zznet_builder::ServerBuilder;
use zznet_transport_tcp::config::TlsConfig;
use zzping_auth::AuthRole;

/// DatabaseNetwork manages the server-side connection using ServerBuilder.
///
/// This abstracts away manual accept loop handling and provides actor-based connection lifecycle management.
pub struct DatabaseNetwork {
    bind_addr: String,
    tls_config: Option<TlsConfig>,
    handshake_timeout: Duration,
}

impl DatabaseNetwork {
    /// Create a new DatabaseNetwork that will use ServerBuilder for accepting connections.
    ///
    /// # Arguments
    /// * `bind_addr` - the address to bind to
    /// * `tls` - optional TLS configuration
    /// * `handshake_timeout` - timeout for the handshake protocol
    pub fn new(bind_addr: &str, tls: Option<TlsConfig>, handshake_timeout: Duration) -> Self {
        Self {
            bind_addr: bind_addr.to_string(),
            tls_config: tls,
            handshake_timeout,
        }
    }

    /// Start the server using ServerBuilder to accept incoming connections.
    ///
    /// This spawns a ServerActor that listens for connections and automatically
    /// spawns HelloActors for each accepted connection. Room handlers are registered
    /// declaratively via the builder.
    ///
    /// # Arguments
    /// * `components` - Started component actors for room handler wiring
    pub async fn run(&self, components: &StartedComponents) -> Result<(), String> {
        // Create room handler factories for all 3 rooms
        use std::sync::Arc as StdArc;

        let intent_factory =
            StdArc::new(crate::room_handlers::IntentConfigRoomHandlerFactory::new(
                components.intent_config.clone(),
            ));

        let memdb_factory = StdArc::new(crate::room_handlers::MemDBRoomHandlerFactory::new(
            components.memdb_addr.clone(),
        ));

        let cstate_factory = StdArc::new(crate::room_handlers::CStateRoomHandlerFactory::new(
            components.cstate.clone(),
        ));

        let builder = ServerBuilder::<DatabaseMessage, AuthRole>::new()
            .bind(&self.bind_addr)
            .as_role(AuthRole::Database)
            .offer_rooms(vec![
                "intent-config".to_string(),
                "memdb".to_string(),
                "query".to_string(),
            ])
            .handshake_timeout(self.handshake_timeout)
            .register_room_handler("intent-config", intent_factory)
            .register_room_handler("memdb", memdb_factory)
            .register_room_handler("query", cstate_factory);

        let builder = if let Some(tls) = &self.tls_config {
            builder.with_tls(tls.clone())
        } else {
            builder
        };

        builder
            .with_default_authorizer(false, None)
            .start()
            .await
            .map(|_| ())
            .map_err(|e| format!("ServerBuilder failed: {:?}", e))
    }
}
