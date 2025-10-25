//! ClientBuilder - Fluent API for creating TCP clients with auto-reconnect

use crate::error::{BuilderError, BuilderResult};
use crate::room_registry::RoomHandlerFactory;
use actix::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use zznet_auth::ApplicationRole;
use zznet_hello::actor::{HelloConfig, start_hello_actor_with_session_manager};
use zznet_hello::connection_manager::ConnectionManager;
use zznet_session::session_manager::SessionManager;
use zznet_session::types::RoomId;
use zznet_transport_tcp::client::TcpTransportClient;
use zznet_transport_tcp::config::TlsConfig;

/// Represents the current state of the ClientActor
#[derive(Debug, Clone, Copy, PartialEq)]
enum ClientState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

/// Builder for creating TCP clients with automatic reconnection
///
///
pub struct ClientBuilder<TRole>
where
    TRole: ApplicationRole,
{
    remote_addr: Option<String>,
    our_role: Option<TRole>,
    offered_rooms: Vec<String>,
    handshake_timeout: Duration,
    reconnect_delay: Duration,
    tls_config: Option<TlsConfig>,
    /// Internal ConnectionManager address - private to builder, not exposed to applications
    _connection_manager: Option<Addr<ConnectionManager<TRole>>>,
    /// Optional authorizer closure used when the builder creates the ConnectionManager.
    authorizer: Option<zznet_auth::acl::GenericAuthorizer<TRole>>,
    // Internal SessionManager owned by the builder when present
    session_manager: Option<Arc<Mutex<SessionManager<TRole>>>>,
    auto_reconnect: bool,
    /// Room handler factories registered via register_room_handler()
    room_handlers: std::collections::HashMap<RoomId, Arc<dyn RoomHandlerFactory<TRole>>>,
    /// Persistent room registry owned by the builder (created lazily)
    room_registry: Option<Arc<Mutex<crate::room_registry::RoomRegistry<TRole>>>>,
}

impl<TRole> ClientBuilder<TRole>
where
    TRole: ApplicationRole,
{
    /// Create a new ClientBuilder with default configuration
    pub fn new() -> Self {
        Self {
            remote_addr: None,
            our_role: None,
            offered_rooms: vec![],
            handshake_timeout: Duration::from_secs(10),
            reconnect_delay: Duration::from_secs(5),
            tls_config: None,
            _connection_manager: None,
            session_manager: None,
            authorizer: None,
            auto_reconnect: true,
            room_handlers: std::collections::HashMap::new(),
            room_registry: None,
        }
    }

    /// Set the remote address to connect to (required)
    ///
    ///
    pub fn connect_to(mut self, addr: impl Into<String>) -> Self {
        self.remote_addr = Some(addr.into());
        self
    }

    /// Set the authentication role for this client
    ///
    ///
    pub fn as_role(mut self, role: TRole) -> Self {
        self.our_role = Some(role);
        self
    }

    /// Set the rooms this client offers
    ///
    ///
    pub fn offer_rooms(mut self, rooms: Vec<String>) -> Self {
        self.offered_rooms = rooms;
        self
    }

    /// Set the handshake timeout duration
    ///
    ///
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// Set the delay between reconnection attempts
    ///
    ///
    pub fn reconnect_delay(mut self, delay: Duration) -> Self {
        self.reconnect_delay = delay;
        self
    }

    /// Set the TLS configuration for encrypted connections
    ///
    ///
    pub fn with_tls(mut self, config: TlsConfig) -> Self {
        self.tls_config = Some(config);
        self
    }

    /// Set TLS configuration from certificate file paths.
    ///
    /// Returns Err(TlsError) if files cannot be read or parsed.
    pub fn with_tls_from_files(
        mut self,
        cert_path: impl Into<String>,
        key_path: impl Into<String>,
        ca_path: Option<impl Into<String>>,
    ) -> Result<Self, zznet_transport_tcp::config::TlsError> {
        let cert = cert_path.into();
        let key = key_path.into();
        let ca_opt = ca_path.map(|p| p.into());
        let ca_ref = ca_opt.as_deref();
        let cfg = TlsConfig::from_file_paths(&cert, &key, ca_ref)?;
        self.tls_config = Some(cfg);
        Ok(self)
    }

    // Removed API: clients should use with_session_manager(...) + with_authorizer(...)

    /// Set an explicit authorizer closure for the builder to use when creating
    /// the internal ConnectionManager. This avoids needing to construct a
    /// ConnectionManager externally.
    pub fn with_authorizer(
        mut self,
        authorizer: zznet_auth::acl::GenericAuthorizer<TRole>,
    ) -> Self {
        self.authorizer = Some(authorizer);
        self
    }

    /// Convenience: create and use the default authorizer (from zznet-auth).
    ///
    /// # Parameters
    /// - `allow_insecure_tcp`: when true, accepts connections without TLS validation
    ///
    /// # Important
    /// The role always comes from the HELLO message. There is no "default role".
    pub fn with_default_authorizer(mut self, allow_insecure_tcp: bool) -> Self {
        let auth = zznet_auth::acl::create_default_authorizer(allow_insecure_tcp);
        self.authorizer = Some(auth);
        self
    }

    /// Expose a SessionManager for advanced cases. If not provided, the builder will
    /// create one internally when `connect()` is called.
    #[deprecated(
        note = "Use the builder's internal SessionManager creation instead. This method will be removed in a future version."
    )]
    pub fn with_session_manager(
        mut self,
        session_manager: Arc<Mutex<SessionManager<TRole>>>,
    ) -> Self {
        self.session_manager = Some(session_manager);
        self
    }

    /// Enable or disable automatic reconnection
    ///
    /// When enabled (default), the client will automatically reconnect
    /// if the connection is lost.
    ///
    ///
    pub fn auto_reconnect(mut self, enable: bool) -> Self {
        self.auto_reconnect = enable;
        self
    }

    /// Register a room handler factory for declarative room setup.
    ///
    /// Handlers registered this way will be automatically wired when the connection
    /// is established and on reconnection.
    pub fn register_room_handler(
        mut self,
        room_id: impl Into<RoomId>,
        factory: Arc<dyn RoomHandlerFactory<TRole>>,
    ) -> Self {
        let room_id = room_id.into();
        self.room_handlers.insert(room_id.clone(), factory.clone());

        // If room_registry already exists, register the handler with it immediately
        if let Some(registry) = &self.room_registry {
            let registry_clone = registry.clone();
            let room_id_clone = room_id.clone();
            // We need to spawn a task to register asynchronously since we can't make this method async
            tokio::spawn(async move {
                let mut reg = registry_clone.lock().await;
                reg.register_room_handler(room_id_clone, factory);
            });
        }

        self
    }

    /// Get or create the persistent RoomRegistry.
    ///
    /// This creates a single, shared RoomRegistry instance that persists across
    /// all connections. The registry can be used for dynamic handler updates.
    ///
    /// # Example
    /// ```
    /// use zznet_builder::client_builder::ClientBuilder;
    /// use zznet_session::room_message_trait::RoomMessageTrait;
    /// use zznet_session::types::RoomId;
    /// use serde::{Deserialize, Serialize};
    ///
    /// #[derive(Debug, Clone)]
    /// enum TestMessages {
    ///     Test,
    /// }
    ///
    /// impl RoomMessageTrait for TestMessages {
    ///     fn room_id(&self) -> RoomId {
    ///         RoomId::from("test")
    ///     }
    ///
    ///     fn serialize_inner(
    ///         &self,
    ///     ) -> Result<Vec<u8>, zznet_session::room_message_trait::SerializationError> {
    ///         Ok(vec![])
    ///     }
    ///
    ///     fn deserialize_for_room(
    ///         _room_id: &RoomId,
    ///         _bytes: &[u8],
    ///     ) -> Result<Self, zznet_session::room_message_trait::DeserializationError> {
    ///         Ok(TestMessages::Test)
    ///     }
    ///
    ///     fn supported_rooms() -> Vec<RoomId> {
    ///         vec![RoomId::from("test")]
    ///     }
    /// }
    ///
    /// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    /// enum TestRole {
    ///     Collector,
    ///     Database,
    /// }
    ///
    /// impl zznet_auth::ApplicationRole for TestRole {
    ///     fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
    ///         match cn {
    ///             "collector" => Ok(TestRole::Collector),
    ///             "database" => Ok(TestRole::Database),
    ///             _ => Err(zznet_auth::error::AuthError::UnknownRole(cn.to_string())),
    ///         }
    ///     }
    ///
    ///     fn as_str(&self) -> &'static str {
    ///         match self {
    ///             TestRole::Collector => "collector",
    ///             TestRole::Database => "database",
    ///         }
    ///     }
    ///
    ///     fn can_connect_to(&self, _target: &Self) -> bool {
    ///         true
    ///     }
    ///
    ///     fn can_access_room(&self, _room_name: &str) -> bool {
    ///         true
    ///     }
    /// }
    ///
    /// let mut builder = ClientBuilder::<TestRole>::new()
    ///     .offer_rooms(vec!["my-room".to_string()]);
    ///
    /// // Access the persistent registry
    /// let registry = builder.room_registry();
    /// // Can add handlers dynamically...
    ///
    /// // builder.connect().await?;
    /// ```
    pub fn room_registry(&mut self) -> Arc<Mutex<crate::room_registry::RoomRegistry<TRole>>> {
        if self.room_registry.is_none() {
            // Get or create SessionManager first
            let sm = self.session_manager();

            // Create the registry
            let mut registry = crate::room_registry::RoomRegistry::new(sm);

            // Register all existing handlers
            for (room_id, factory) in &self.room_handlers {
                registry.register_room_handler(room_id.clone(), factory.clone());
            }

            self.room_registry = Some(Arc::new(Mutex::new(registry)));
        }

        self.room_registry.clone().unwrap()
    }

    /// Get a reference to the SessionManager.
    ///
    /// If the SessionManager has not been created yet, this will create it
    /// using the offered rooms configured on the builder.
    ///
    /// This allows for app-level room wiring before calling `connect()`.
    ///
    /// # Example
    /// ```
    /// use zznet_builder::client_builder::ClientBuilder;
    /// use zznet_session::room_message_trait::RoomMessageTrait;
    /// use zznet_session::types::RoomId;
    /// use serde::{Deserialize, Serialize};
    ///
    /// #[derive(Debug, Clone)]
    /// enum TestMessages {
    ///     Test,
    /// }
    ///
    /// impl RoomMessageTrait for TestMessages {
    ///     fn room_id(&self) -> RoomId {
    ///         RoomId::from("test")
    ///     }
    ///
    ///     fn serialize_inner(
    ///         &self,
    ///     ) -> Result<Vec<u8>, zznet_session::room_message_trait::SerializationError> {
    ///         Ok(vec![])
    ///     }
    ///
    ///     fn deserialize_for_room(
    ///         _room_id: &RoomId,
    ///         _bytes: &[u8],
    ///     ) -> Result<Self, zznet_session::room_message_trait::DeserializationError> {
    ///         Ok(TestMessages::Test)
    ///     }
    ///
    ///     fn supported_rooms() -> Vec<RoomId> {
    ///         vec![RoomId::from("test")]
    ///     }
    /// }
    ///
    /// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    /// enum TestRole {
    ///     Collector,
    ///     Database,
    /// }
    ///
    /// impl zznet_auth::ApplicationRole for TestRole {
    ///     fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
    ///         match cn {
    ///             "collector" => Ok(TestRole::Collector),
    ///             "database" => Ok(TestRole::Database),
    ///             _ => Err(zznet_auth::error::AuthError::UnknownRole(cn.to_string())),
    ///         }
    ///     }
    ///
    ///     fn as_str(&self) -> &'static str {
    ///         match self {
    ///             TestRole::Collector => "collector",
    ///             TestRole::Database => "database",
    ///         }
    ///     }
    ///
    ///     fn can_connect_to(&self, _target: &Self) -> bool {
    ///         true
    ///     }
    ///
    ///     fn can_access_room(&self, _room_name: &str) -> bool {
    ///         true
    ///     }
    /// }
    ///
    /// let mut builder = ClientBuilder::<TestRole>::new()
    ///     .offer_rooms(vec!["my-room".to_string()]);
    ///
    /// // Access SessionManager before connecting
    /// let session_manager = builder.session_manager();
    /// // Perform advanced wiring...
    ///
    /// // Then connect
    /// // builder.connect().await?;
    /// ```
    pub fn session_manager(&mut self) -> Arc<Mutex<SessionManager<TRole>>> {
        if self.session_manager.is_none() {
            // Create SessionManager with offered rooms
            let offered: Vec<zznet_session::types::RoomId> = self
                .offered_rooms
                .iter()
                .map(|r| zznet_session::types::RoomId::from(r.as_str()))
                .collect();
            let sm = SessionManager::<TRole>::new(offered);
            self.session_manager = Some(Arc::new(Mutex::new(sm)));
        }
        self.session_manager.clone().unwrap()
    }

    /// Validate configuration before connecting
    fn validate(&self) -> BuilderResult<()> {
        if self.remote_addr.is_none() {
            return Err(BuilderError::MissingConfig("remote_addr".to_string()));
        }

        if self.offered_rooms.is_empty() {
            return Err(BuilderError::InvalidConfig(
                "must offer at least one room".to_string(),
            ));
        }

        Ok(())
    }

    /// Connect to the server and return a handle
    ///
    /// This creates a ClientActor that manages the connection and will
    /// automatically reconnect if the connection is lost (unless disabled).
    pub async fn connect(self) -> BuilderResult<Addr<ClientActor<TRole>>> {
        // Delegate to the richer API that returns the ConnectionManager as well,
        // but keep the original signature by returning only the actor Addr.
        let (actor_addr, _cm_addr) = self.connect_with_connection_manager().await?;
        Ok(actor_addr)
    }

    /// Connect and also return the ConnectionManager Addr created by the builder.
    ///
    /// This is useful for tests that need to inspect peers, subscribe to inbound
    /// messages, or send messages via the ConnectionManager API.
    ///
    /// **WARNING:** This is an internal test API and should not be used in production code.
    /// It exposes protocol-level details that may change. Use the public `connect()` method instead.
    #[doc(hidden)]
    pub async fn connect_with_connection_manager(
        self,
    ) -> BuilderResult<(Addr<ClientActor<TRole>>, Addr<ConnectionManager<TRole>>)> {
        self.validate()?;

        let remote_addr = self.remote_addr.unwrap();

        // Ensure we have a SessionManager: use provided or create one
        let session_manager = match &self.session_manager {
            Some(sm) => sm.clone(),
            None => {
                // Create a default SessionManager with offered rooms (convert to RoomId)
                let offered: Vec<zznet_session::types::RoomId> = self
                    .offered_rooms
                    .iter()
                    .map(|r| zznet_session::types::RoomId::from(r.as_str()))
                    .collect();
                let sm = SessionManager::<TRole>::new(offered);
                Arc::new(Mutex::new(sm))
            }
        };

        // Create or use provided ConnectionManager (we removed direct supply API)
        let connection_manager_addr = match self._connection_manager {
            Some(cm) => cm,
            None => {
                let authorizer = match self.authorizer {
                    Some(a) => a,
                    None => Box::new(|_ctx: &zznet_api::types::AuthContext| None),
                };
                let mut cm =
                    zznet_hello::connection_manager::ConnectionManager::new_with_session_manager(
                        session_manager.clone(),
                        authorizer,
                    );

                // Wire room handlers if any are registered
                if !self.room_handlers.is_empty() {
                    // Get or create the persistent room registry
                    let registry = if let Some(reg) = &self.room_registry {
                        reg.clone()
                    } else {
                        // Create the registry if it doesn't exist yet
                        let mut registry_instance =
                            crate::room_registry::RoomRegistry::new(session_manager.clone());
                        for (room_id, factory) in &self.room_handlers {
                            registry_instance
                                .register_room_handler(room_id.clone(), factory.clone());
                        }
                        Arc::new(Mutex::new(registry_instance))
                    };

                    let wirer: Arc<
                        dyn Fn(
                                Arc<Mutex<SessionManager<TRole>>>,
                                &zznet_session::types::PeerId,
                            ) -> std::pin::Pin<
                                Box<dyn std::future::Future<Output = Result<(), String>> + Send>,
                            > + Send
                            + Sync,
                    > = Arc::new(move |_sm, peer_id| {
                        let peer_id = peer_id.clone();
                        let registry = registry.clone();
                        Box::pin(async move {
                            // Use the persistent RoomRegistry to wire this peer
                            let registry_lock = registry.lock().await;
                            registry_lock
                                .wire_peer(&peer_id)
                                .await
                                .map_err(|e| format!("Failed to wire peer: {}", e))
                        })
                    });
                    cm = cm.with_room_handler_wirer(wirer);
                }

                cm.start()
            }
        };

        // our_role must be set by caller
        let role = match self.our_role {
            Some(r) => r,
            None => return Err(BuilderError::MissingConfig("our_role".to_string())),
        };

        // Create HelloConfig
        let hello_config = HelloConfig {
            our_role: role.as_str().to_string(),
            offered_rooms: self.offered_rooms.clone(),
            handshake_timeout: self.handshake_timeout,
            hostname: "client-hostname".to_string(),
        };

        // Create and start ClientActor
        let actor = ClientActor {
            remote_addr,
            tls_config: self.tls_config,
            hello_config,
            connection_manager: connection_manager_addr.clone(),
            reconnect_delay: self.reconnect_delay,
            auto_reconnect: self.auto_reconnect,
            current_hello_actor: None,
            state: ClientState::Disconnected,
        };

        Ok((actor.start(), connection_manager_addr))
    }
}

impl<TRole> Default for ClientBuilder<TRole>
where
    TRole: ApplicationRole,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Actor that manages the client's connection loop
pub struct ClientActor<TRole>
where
    TRole: ApplicationRole,
{
    remote_addr: String,
    tls_config: Option<TlsConfig>,
    hello_config: HelloConfig,
    connection_manager: Addr<ConnectionManager<TRole>>,
    reconnect_delay: Duration,
    auto_reconnect: bool,
    current_hello_actor: Option<Addr<zznet_hello::actor::HelloActor>>,
    state: ClientState,
}

impl<TRole> Actor for ClientActor<TRole>
where
    TRole: ApplicationRole,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        tracing::info!("ClientActor started, initiating connection");
        self.state = ClientState::Disconnected;
        self.connect_loop(ctx);
    }
}

impl<TRole> ClientActor<TRole>
where
    TRole: ApplicationRole,
{
    /// Run the connection loop
    fn connect_loop(&mut self, ctx: &mut Context<Self>) {
        self.state = ClientState::Connecting;
        let remote_addr = self.remote_addr.clone();
        let tls_config = self.tls_config.clone();
        let hello_config = self.hello_config.clone();
        let connection_manager = self.connection_manager.clone();

        let fut = async move {
            use zznet_api::transport::TransportClient;

            let client_result = TcpTransportClient::new(remote_addr.clone(), tls_config);

            let client = match client_result {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to create TCP client: {}", e);
                    return ConnectionResult::Failed;
                }
            };

            match client.connect().await {
                Ok(transport) => {
                    tracing::info!("Connected to {}", remote_addr);
                    // Integration tests look for this phrase specifically on the client side
                    tracing::info!("Connected to peer");

                    // Spawn HelloActor for this connection
                    let hello_actor = start_hello_actor_with_session_manager(
                        transport,
                        hello_config,
                        Some(connection_manager.recipient()),
                    );

                    ConnectionResult::Connected(hello_actor)
                }
                Err(e) => {
                    tracing::error!("Connection to {} failed: {}", remote_addr, e);
                    ConnectionResult::Failed
                }
            }
        };

        ctx.spawn(fut.into_actor(self).map(|result, act, ctx| match result {
            ConnectionResult::Connected(hello_actor) => {
                act.current_hello_actor = Some(hello_actor);
                act.state = ClientState::Connected;
                // TODO: Monitor HelloActor and reconnect if it stops
            }
            ConnectionResult::Failed => {
                act.state = ClientState::Disconnected;
                if act.auto_reconnect {
                    tracing::info!("Will retry connection in {:?}", act.reconnect_delay);
                    ctx.run_later(act.reconnect_delay, |act, ctx| {
                        act.connect_loop(ctx);
                    });
                } else {
                    tracing::info!("Auto-reconnect disabled, stopping");
                    ctx.stop();
                }
            }
        }));
    }
}

/// Internal message for connection results
enum ConnectionResult {
    Connected(Addr<zznet_hello::actor::HelloActor>),
    Failed,
}

/// Message to disconnect the client cleanly
///
/// Sends this message to the ClientActor to close the connection and stop the actor.
/// Auto-reconnection will be disabled.
///
/// # Example
/// ```
/// // client_addr.send(zznet_builder::Disconnect).await.ok();
/// ```
#[derive(Message)]
#[rtype(result = "()")]
pub struct Disconnect;

impl<TRole> Handler<Disconnect> for ClientActor<TRole>
where
    TRole: ApplicationRole,
{
    type Result = ();

    fn handle(&mut self, _msg: Disconnect, ctx: &mut Context<Self>) {
        tracing::info!("Disconnecting client");
        self.state = ClientState::Disconnecting;
        self.auto_reconnect = false;

        if let Some(hello_actor) = &self.current_hello_actor {
            hello_actor.do_send(zznet_hello::actor::Disconnect);
        }

        ctx.stop();
    }
}

/// Message to manually trigger a reconnection
///
/// Sends this message to the ClientActor to force an immediate reconnection attempt.
/// This is useful when you want to reconnect without waiting for the automatic reconnect delay.
///
/// # Example
/// ```
/// // client_addr.send(zznet_builder::Reconnect).await.ok();
/// ```
#[derive(Message)]
#[rtype(result = "()")]
pub struct Reconnect;

impl<TRole> Handler<Reconnect> for ClientActor<TRole>
where
    TRole: ApplicationRole,
{
    type Result = ();

    fn handle(&mut self, _msg: Reconnect, ctx: &mut Context<Self>) {
        tracing::info!("Manual reconnection triggered");

        match self.state {
            ClientState::Connecting => {
                tracing::warn!("Reconnect ignored: already connecting");
            }
            ClientState::Connected => {
                // Disconnect first, then reconnect
                if let Some(hello_actor) = &self.current_hello_actor {
                    hello_actor.do_send(zznet_hello::actor::Disconnect);
                }
                self.state = ClientState::Disconnecting;
                self.current_hello_actor = None;
                // Wait 100ms for termination, then reconnect
                ctx.run_later(Duration::from_millis(100), |act, ctx| {
                    act.connect_loop(ctx);
                });
            }
            ClientState::Disconnected | ClientState::Disconnecting => {
                // Already disconnected or disconnecting, just start connecting
                self.connect_loop(ctx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    /// Minimal test role for builder unit tests. Derive traits required by ApplicationRole.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    enum TestRole {
        Collector,
        Database,
    }

    impl zznet_auth::ApplicationRole for TestRole {
        fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
            match cn {
                "collector" => Ok(TestRole::Collector),
                "database" => Ok(TestRole::Database),
                _ => Err(zznet_auth::error::AuthError::UnknownRole(cn.to_string())),
            }
        }

        fn as_str(&self) -> &'static str {
            match self {
                TestRole::Collector => "collector",
                TestRole::Database => "database",
            }
        }

        fn can_connect_to(&self, _target: &Self) -> bool {
            true
        }

        fn can_access_room(&self, _room_name: &str) -> bool {
            true
        }
    }

    #[test]
    fn test_client_builder_new() {
        let builder = ClientBuilder::<TestRole>::new();
        assert!(builder.remote_addr.is_none());
        assert!(builder.our_role.is_none());
        assert!(builder.offered_rooms.is_empty());
        assert!(builder.auto_reconnect);
    }

    #[test]
    fn test_client_builder_fluent_api() {
        let builder = ClientBuilder::<TestRole>::new()
            .connect_to("127.0.0.1:8080")
            .as_role(TestRole::Database)
            .offer_rooms(vec!["test".to_string()])
            .handshake_timeout(Duration::from_secs(5))
            .reconnect_delay(Duration::from_secs(10))
            .auto_reconnect(false);

        assert_eq!(builder.remote_addr.unwrap(), "127.0.0.1:8080");
        assert_eq!(builder.our_role.unwrap(), TestRole::Database);
        assert_eq!(builder.offered_rooms, vec!["test".to_string()]);
        assert_eq!(builder.handshake_timeout, Duration::from_secs(5));
        assert_eq!(builder.reconnect_delay, Duration::from_secs(10));
        assert!(!builder.auto_reconnect);
    }

    #[test]
    fn test_validation_missing_remote_addr() {
        let builder = ClientBuilder::<TestRole>::new().offer_rooms(vec!["test".to_string()]);

        let result = builder.validate();
        assert!(matches!(result, Err(BuilderError::MissingConfig(_))));
    }

    #[test]
    fn test_validation_missing_rooms() {
        let builder = ClientBuilder::<TestRole>::new().connect_to("127.0.0.1:8080");

        let result = builder.validate();
        assert!(matches!(result, Err(BuilderError::InvalidConfig(_))));
    }

    #[test]
    fn test_validation_success() {
        let builder = ClientBuilder::<TestRole>::new()
            .connect_to("127.0.0.1:8080")
            .offer_rooms(vec!["test".to_string()]);

        let result = builder.validate();
        assert!(result.is_ok());
    }
}
