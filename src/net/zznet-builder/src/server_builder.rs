//! ServerBuilder - Fluent API for creating TCP servers

use crate::error::{BuilderError, BuilderResult};
use actix::prelude::*;
use std::time::Duration;
use zznet_auth::ApplicationRole;
use zznet_hello::actor::{HelloConfig, start_hello_actor_with_session_manager};
use zznet_hello::connection_manager::ConnectionManager;
use zznet_session::room_message_trait::RoomMessageTrait;
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
    connection_manager: Option<Addr<ConnectionManager<TMsg, TRole>>>,
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
            connection_manager: None,
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

    /// Set the ConnectionManager for handling connections
    ///
    /// If not provided, a new ConnectionManager will be created.
    pub fn with_connection_manager(
        mut self,
        manager: Addr<ConnectionManager<TMsg, TRole>>,
    ) -> Self {
        self.connection_manager = Some(manager);
        self
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
    pub async fn start(self) -> BuilderResult<Addr<ServerActor<TMsg, TRole>>> {
        self.validate()?;

        let bind_addr = self.bind_addr.unwrap();

        // SECURITY: ConnectionManager is REQUIRED and must be provided with an authorizer.
        // Applications must create and configure the ConnectionManager themselves,
        // then pass it via with_connection_manager(). This ensures:
        // - Authorizer is always configured (no unauthenticated connections possible)
        // - Application controls authorization policy
        let connection_manager = self.connection_manager.ok_or_else(|| {
            BuilderError::MissingConfig(
                "ConnectionManager with authorizer is REQUIRED. Use with_connection_manager() to provide one.".to_string(),
            )
        })?;

        // Create TCP server
        let tcp_server = TcpTransportServer::new(&bind_addr, self.tls_config)
            .await
            .map_err(|e| BuilderError::BindFailed(format!("{}: {}", bind_addr, e)))?;

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
            tcp_server: std::sync::Arc::new(tokio::sync::Mutex::new(tcp_server)),
            hello_config,
            connection_manager,
        };

        Ok(actor.start())
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
pub struct ServerActor<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    tcp_server: std::sync::Arc<tokio::sync::Mutex<TcpTransportServer>>,
    hello_config: HelloConfig,
    connection_manager: Addr<ConnectionManager<TMsg, TRole>>,
}

impl<TMsg, TRole> Actor for ServerActor<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        tracing::info!("ServerActor started, beginning accept loop");
        // Trigger first accept
        ctx.address().do_send(AcceptNext);
    }
}

/// Internal message to trigger the next accept
#[derive(Message)]
#[rtype(result = "()")]
struct AcceptNext;

impl<TMsg, TRole> Handler<AcceptNext> for ServerActor<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
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
                        tracing::info!("Accepted new connection");

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
#[derive(Message)]
#[rtype(result = "()")]
pub struct StopServer;

impl<TMsg, TRole> Handler<StopServer> for ServerActor<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    type Result = ();

    fn handle(&mut self, _msg: StopServer, ctx: &mut Context<Self>) {
        tracing::info!("Stopping server");
        ctx.stop();
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

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
