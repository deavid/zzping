//! ServerBuilder - Fluent API for creating TCP servers

use crate::error::{BuilderError, BuilderResult};
use crate::room_registry::RoomHandlerFactory;
use actix::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use zznet_auth::ApplicationRole;
use zznet_hello::actor::{HelloConfig, start_hello_actor_with_session_manager};
use zznet_hello::connection_manager::ConnectionManager;
use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::session_manager::SessionManager;
use zznet_session::types::RoomId;
use zznet_transport_tcp::config::TlsConfig;
use zznet_transport_tcp::server::TcpTransportServer;

/// Builder for creating TCP servers with automatic connection management
///
///
pub struct ServerBuilder<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    bind_addr: Option<String>,
    our_role: Option<TRole>,
    offered_rooms: Vec<String>,
    handshake_timeout: Duration,
    tls_config: Option<TlsConfig>,
    /// Internal ConnectionManager address - private to builder, not exposed to applications
    _connection_manager: Option<Addr<ConnectionManager<TRole>>>,
    // Internal SessionManager owned by the builder when present
    session_manager: Option<Arc<Mutex<SessionManager<TRole>>>>,
    /// Optional authorizer closure used when the builder creates the ConnectionManager.
    authorizer: Option<zznet_auth::acl::GenericAuthorizer<TRole>>,
    room_handlers: std::collections::HashMap<RoomId, Arc<dyn RoomHandlerFactory<TMsg, TRole>>>,
    /// Persistent room registry owned by the builder (created lazily)
    room_registry: Option<Arc<Mutex<crate::room_registry::RoomRegistry<TMsg, TRole>>>>,
}

impl<TMsg, TRole> ServerBuilder<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    /// Create a new ServerBuilder with default configuration
    pub fn new() -> Self {
        Self {
            bind_addr: None,
            our_role: None,
            offered_rooms: vec![],
            handshake_timeout: Duration::from_secs(10),
            tls_config: None,
            _connection_manager: None,
            session_manager: None,
            authorizer: None,
            room_handlers: std::collections::HashMap::new(),
            room_registry: None,
        }
    }

    /// Set the bind address (required)
    ///
    ///
    pub fn bind(mut self, addr: impl Into<String>) -> Self {
        self.bind_addr = Some(addr.into());
        self
    }

    /// Set the authentication role for this server
    ///
    ///
    pub fn as_role(mut self, role: TRole) -> Self {
        self.our_role = Some(role);
        self
    }

    /// Set the rooms this server offers
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

    // Removed API: Use with_session_manager(...) + with_authorizer(...)

    /// Set an explicit authorizer closure for the builder to use when creating
    /// the internal ConnectionManager.
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
    /// create one internally when `start()` is called.
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

    /// Register a room handler factory for declarative room setup.
    ///
    /// Handlers registered this way will be automatically wired when the connection
    /// is established and on reconnection.
    pub fn register_room_handler(
        mut self,
        room_id: impl Into<RoomId>,
        factory: Arc<dyn RoomHandlerFactory<TMsg, TRole>>,
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
    /// use zznet_builder::server_builder::ServerBuilder;
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
    /// let mut builder = ServerBuilder::<TestMessages, TestRole>::new()
    ///     .offer_rooms(vec!["my-room".to_string()]);
    ///
    /// // Access the persistent registry
    /// let registry = builder.room_registry();
    /// // Can add handlers dynamically...
    ///
    /// // builder.start().await?;
    /// ```
    pub fn room_registry(&mut self) -> Arc<Mutex<crate::room_registry::RoomRegistry<TMsg, TRole>>> {
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
    /// This allows for app-level room wiring before calling `start()`.
    ///
    /// # Example
    /// ```
    /// use zznet_builder::server_builder::ServerBuilder;
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
    /// let mut builder = ServerBuilder::<TestMessages, TestRole>::new()
    ///     .offer_rooms(vec!["my-room".to_string()]);
    ///
    /// // Access SessionManager before starting
    /// let session_manager = builder.session_manager();
    /// // Perform advanced wiring...
    ///
    /// // Then start the server
    /// // builder.start().await?;
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

    /// Validate configuration before starting
    fn validate(&self) -> BuilderResult<()> {
        if self.bind_addr.is_none() {
            return Err(BuilderError::MissingConfig("bind_addr".to_string()));
        }

        if self.offered_rooms.is_empty() {
            return Err(BuilderError::InvalidConfig(
                "must offer at least one room".to_string(),
            ));
        }

        Ok(())
    }

    /// Start the server and return a handle
    ///
    /// This creates a ServerActor that listens for connections and spawns
    /// HelloActors for each accepted connection.
    pub async fn start(self) -> BuilderResult<Addr<ServerActor<TRole>>> {
        self.validate()?;

        let bind_addr = self.bind_addr.unwrap();

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

        // Ensure we have a ConnectionManager: use provided or create one bound to session_manager
        let connection_manager = match self._connection_manager {
            Some(cm) => cm,
            None => {
                // Use the builder-provided authorizer if present, otherwise use a conservative default
                // that rejects all connections. ConnectionManager requires an authorizer.
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

        // Create TCP server
        let tcp_server = TcpTransportServer::new(&bind_addr, self.tls_config)
            .await
            .map_err(|e| BuilderError::BindFailed(format!("{}: {}", bind_addr, e)))?;

        // Get the actual bound address (useful for ephemeral ports)
        let actual_bind_addr = tcp_server
            .local_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|_| bind_addr.clone());

        // Create HelloConfig
        let role = match self.our_role {
            Some(r) => r,
            None => return Err(BuilderError::MissingConfig("our_role".to_string())),
        };

        let hello_config = HelloConfig {
            our_role: role.as_str().to_string(),
            offered_rooms: self.offered_rooms.clone(),
            handshake_timeout: self.handshake_timeout,
            hostname: "server-hostname".to_string(),
        };

        // Create and start ServerActor
        let actor = ServerActor {
            tcp_server: Arc::new(Mutex::new(tcp_server)),
            hello_config,
            connection_manager,
            bind_addr: actual_bind_addr,
        };

        Ok(actor.start())
    }

    /// Start the server and also return the ConnectionManager Addr created by the builder.
    ///
    /// **WARNING:** This is an internal test API and should not be used in production code.
    /// It exposes protocol-level details that may change. Use the public `start()` method instead.
    #[doc(hidden)]
    pub async fn start_with_connection_manager(
        self,
    ) -> BuilderResult<(Addr<ServerActor<TRole>>, Addr<ConnectionManager<TRole>>)> {
        self.validate()?;

        let bind_addr = self.bind_addr.unwrap();

        // Ensure we have a SessionManager: use provided or create one
        let session_manager = match &self.session_manager {
            Some(sm) => sm.clone(),
            None => {
                let offered: Vec<zznet_session::types::RoomId> = self
                    .offered_rooms
                    .iter()
                    .map(|r| zznet_session::types::RoomId::from(r.as_str()))
                    .collect();
                let sm = SessionManager::<TRole>::new(offered);
                Arc::new(Mutex::new(sm))
            }
        };

        // Create ConnectionManager
        let connection_manager = match self._connection_manager {
            Some(cm) => cm,
            None => {
                let authorizer = match self.authorizer {
                    Some(a) => a,
                    None => Box::new(|_ctx: &zznet_api::types::AuthContext| None),
                };

                let cm =
                    zznet_hello::connection_manager::ConnectionManager::new_with_session_manager(
                        session_manager.clone(),
                        authorizer,
                    );
                cm.start()
            }
        };

        // Create TCP server
        let tcp_server = TcpTransportServer::new(&bind_addr, self.tls_config)
            .await
            .map_err(|e| BuilderError::BindFailed(format!("{}: {}", bind_addr, e)))?;

        // Get the actual bound address (useful for ephemeral ports)
        let actual_bind_addr = tcp_server
            .local_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|_| bind_addr.clone());

        // Create HelloConfig
        let role = match self.our_role {
            Some(r) => r,
            None => return Err(BuilderError::MissingConfig("our_role".to_string())),
        };

        let hello_config = HelloConfig {
            our_role: role.as_str().to_string(),
            offered_rooms: self.offered_rooms.clone(),
            handshake_timeout: self.handshake_timeout,
            hostname: "server-hostname".to_string(),
        };

        // Create and start ServerActor
        let actor = ServerActor {
            tcp_server: Arc::new(Mutex::new(tcp_server)),
            hello_config,
            connection_manager: connection_manager.clone(),
            bind_addr: actual_bind_addr,
        };

        Ok((actor.start(), connection_manager))
    }
}

impl<TMsg, TRole> Default for ServerBuilder<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Actor that manages the server's accept loop
pub struct ServerActor<TRole>
where
    TRole: ApplicationRole,
{
    tcp_server: Arc<Mutex<TcpTransportServer>>,
    hello_config: HelloConfig,
    connection_manager: Addr<ConnectionManager<TRole>>,
    /// Cached bind address to avoid locking tcp_server (which may be blocked in accept())
    bind_addr: String,
}

impl<TRole> Actor for ServerActor<TRole>
where
    TRole: ApplicationRole,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        tracing::info!("ServerActor started, beginning accept loop");
        tracing::info!("Database service ready - using ConnectionManager for connections");
        // Trigger first accept
        ctx.address().do_send(AcceptNext);
    }
}

/// Internal message to trigger the next accept
#[derive(Message)]
#[rtype(result = "()")]
struct AcceptNext;

impl<TRole> Handler<AcceptNext> for ServerActor<TRole>
where
    TRole: ApplicationRole,
{
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, _msg: AcceptNext, _ctx: &mut Context<Self>) -> Self::Result {
        use zznet_api::transport::TransportServer;

        let hello_config = self.hello_config.clone();
        let connection_manager = self.connection_manager.clone();
        let tcp_server = self.tcp_server.clone();

        Box::pin(
            async move {
                // Lock and accept the next connection
                let mut server = tcp_server.lock().await;
                server.accept().await
            }
            .into_actor(self)
            .map(move |result, _act, ctx| {
                match result {
                    Ok(transport) => {
                        // Try to print peer addr where possible (plain TCP prints it inside accept)
                        tracing::info!("Accepted connection from peer");

                        // Spawn HelloActor for this connection
                        let _hello_actor = start_hello_actor_with_session_manager(
                            transport,
                            hello_config,
                            Some(connection_manager.recipient()),
                        );

                        // HelloActor will notify ConnectionManager when handshake completes
                    }
                    Err(e) => {
                        tracing::error!("Accept failed: {}", e);
                    }
                }

                // Trigger next accept
                ctx.address().do_send(AcceptNext);
            }),
        )
    }
}

/// Message to stop the server
///
/// Sends this message to the ServerActor to gracefully shut down the server.
/// This will close the listening socket and stop accepting new connections.
/// Existing connections will be allowed to complete their current operations.
///
/// # Example
/// ```
/// // server_addr.send(zznet_builder::StopServer).await.ok();
/// ```
#[derive(Message)]
#[rtype(result = "()")]
pub struct StopServer;

impl<TRole> Handler<StopServer> for ServerActor<TRole>
where
    TRole: ApplicationRole,
{
    type Result = ();

    fn handle(&mut self, _msg: StopServer, ctx: &mut Context<Self>) {
        tracing::info!("Stopping server");
        ctx.stop();
    }
}

/// Message to request the server's bound address
///
/// Sends this message to the ServerActor to get the actual listening address.
/// This is particularly useful when binding to port 0 (ephemeral port), as it
/// returns the actual assigned port.
///
/// # Example
/// ```
/// // let addr = server_addr.send(zznet_builder::GetBindAddr).await??;
/// // println!("Server listening on: {}", addr);
/// ```
#[derive(Message)]
#[rtype(result = "Result<String, ()>")]
pub struct GetBindAddr;

impl<TRole> Handler<GetBindAddr> for ServerActor<TRole>
where
    TRole: ApplicationRole,
{
    type Result = Result<String, ()>;

    fn handle(&mut self, _msg: GetBindAddr, _ctx: &mut Context<Self>) -> Self::Result {
        // Return the cached bind address instead of locking tcp_server
        // (which may be blocked in accept())
        Ok(self.bind_addr.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use zznet_session::room_message_trait::RoomMessageTrait;
    use zznet_session::types::RoomId;

    /// Minimal test message enum used by the server builder tests.
    #[derive(Debug, Clone)]
    enum TestMessages {
        /// Simple unit test message.
        Test,
    }

    impl RoomMessageTrait for TestMessages {
        fn room_id(&self) -> RoomId {
            RoomId::from("test")
        }

        fn serialize_inner(
            &self,
        ) -> Result<Vec<u8>, zznet_session::room_message_trait::SerializationError> {
            Ok(vec![])
        }

        fn deserialize_for_room(
            _room_id: &RoomId,
            _bytes: &[u8],
        ) -> Result<Self, zznet_session::room_message_trait::DeserializationError> {
            Ok(TestMessages::Test)
        }

        fn supported_rooms() -> Vec<RoomId> {
            vec![RoomId::from("test")]
        }
    }

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
    fn test_server_builder_new() {
        let builder = ServerBuilder::<TestMessages, TestRole>::new();
        assert!(builder.bind_addr.is_none());
        assert!(builder.our_role.is_none());
        assert!(builder.offered_rooms.is_empty());
    }

    #[test]
    fn test_server_builder_fluent_api() {
        let builder = ServerBuilder::<TestMessages, TestRole>::new()
            .bind("127.0.0.1:8080")
            .as_role(TestRole::Collector)
            .offer_rooms(vec!["test".to_string()])
            .handshake_timeout(Duration::from_secs(5));

        assert_eq!(builder.bind_addr.unwrap(), "127.0.0.1:8080");
        assert_eq!(builder.our_role.unwrap(), TestRole::Collector);
        assert_eq!(builder.offered_rooms, vec!["test".to_string()]);
        assert_eq!(builder.handshake_timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_validation_missing_bind_addr() {
        let builder =
            ServerBuilder::<TestMessages, TestRole>::new().offer_rooms(vec!["test".to_string()]);

        let result = builder.validate();
        assert!(matches!(result, Err(BuilderError::MissingConfig(_))));
    }

    #[test]
    fn test_validation_missing_rooms() {
        let builder = ServerBuilder::<TestMessages, TestRole>::new().bind("127.0.0.1:8080");

        let result = builder.validate();
        assert!(matches!(result, Err(BuilderError::InvalidConfig(_))));
    }

    #[test]
    fn test_validation_success() {
        let builder = ServerBuilder::<TestMessages, TestRole>::new()
            .bind("127.0.0.1:8080")
            .offer_rooms(vec!["test".to_string()]);

        let result = builder.validate();
        assert!(result.is_ok());
    }
}
