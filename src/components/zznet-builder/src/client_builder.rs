//! ClientBuilder - Fluent API for creating TCP clients with auto-reconnect

use crate::error::{BuilderError, BuilderResult};
use actix::prelude::*;
use std::time::Duration;
use zznet_hello::actor::{HelloConfig, start_hello_actor_with_session_manager};
use zznet_hello::auth::AuthRole;
use zznet_hello::connection_manager::ConnectionManager;
use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::types::RoomId;
use zznet_transport_tcp::client::TcpTransportClient;
use zznet_transport_tcp::config::TlsConfig;

/// Builder for creating TCP clients with automatic reconnection
///
///
pub struct ClientBuilder<TMsg>
where
    TMsg: RoomMessageTrait,
{
    remote_addr: Option<String>,
    our_role: AuthRole,
    offered_rooms: Vec<String>,
    handshake_timeout: Duration,
    reconnect_delay: Duration,
    tls_config: Option<TlsConfig>,
    connection_manager: Option<Addr<ConnectionManager<TMsg>>>,
    auto_reconnect: bool,
}

impl<TMsg> ClientBuilder<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
{
    /// Create a new ClientBuilder with default configuration
    pub fn new() -> Self {
        Self {
            remote_addr: None,
            our_role: AuthRole::Collector,
            offered_rooms: vec![],
            handshake_timeout: Duration::from_secs(10),
            reconnect_delay: Duration::from_secs(5),
            tls_config: None,
            connection_manager: None,
            auto_reconnect: true,
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
    pub fn as_role(mut self, role: AuthRole) -> Self {
        self.our_role = role;
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

    /// Set the ConnectionManager for handling connections
    ///
    /// If not provided, a new ConnectionManager will be created.
    pub fn with_connection_manager(mut self, manager: Addr<ConnectionManager<TMsg>>) -> Self {
        self.connection_manager = Some(manager);
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
    pub async fn connect(self) -> BuilderResult<Addr<ClientActor<TMsg>>> {
        self.validate()?;

        let remote_addr = self.remote_addr.unwrap();

        // Create ConnectionManager if not provided
        let connection_manager = if let Some(cm) = self.connection_manager {
            cm
        } else {
            let rooms: Vec<RoomId> = self
                .offered_rooms
                .iter()
                .map(|s| RoomId::from(s.as_str()))
                .collect();
            ConnectionManager::<TMsg>::new(rooms).start()
        };

        // Create HelloConfig
        let hello_config = HelloConfig {
            our_role: self.our_role,
            offered_rooms: self.offered_rooms.clone(),
            handshake_timeout: self.handshake_timeout,
            hostname: "client-hostname".to_string(),
        };

        // Create and start ClientActor
        let actor = ClientActor {
            remote_addr,
            tls_config: self.tls_config,
            hello_config,
            connection_manager,
            reconnect_delay: self.reconnect_delay,
            auto_reconnect: self.auto_reconnect,
            current_hello_actor: None,
        };

        Ok(actor.start())
    }
}

impl<TMsg> Default for ClientBuilder<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Actor that manages the client's connection loop
pub struct ClientActor<TMsg>
where
    TMsg: RoomMessageTrait,
{
    remote_addr: String,
    tls_config: Option<TlsConfig>,
    hello_config: HelloConfig,
    connection_manager: Addr<ConnectionManager<TMsg>>,
    reconnect_delay: Duration,
    auto_reconnect: bool,
    current_hello_actor: Option<Addr<zznet_hello::actor::HelloActor>>,
}

impl<TMsg> Actor for ClientActor<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        tracing::info!("ClientActor started, initiating connection");
        self.connect_loop(ctx);
    }
}

impl<TMsg> ClientActor<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
{
    /// Run the connection loop
    fn connect_loop(&mut self, ctx: &mut Context<Self>) {
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
                // TODO: Monitor HelloActor and reconnect if it stops
            }
            ConnectionResult::Failed => {
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

/// Message to disconnect the client
#[derive(Message)]
#[rtype(result = "()")]
pub struct Disconnect;

impl<TMsg> Handler<Disconnect> for ClientActor<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
{
    type Result = ();

    fn handle(&mut self, _msg: Disconnect, ctx: &mut Context<Self>) {
        tracing::info!("Disconnecting client");
        self.auto_reconnect = false;

        if let Some(hello_actor) = &self.current_hello_actor {
            hello_actor.do_send(zznet_hello::actor::Disconnect);
        }

        ctx.stop();
    }
}

/// Message to manually trigger a reconnection
#[derive(Message)]
#[rtype(result = "()")]
pub struct Reconnect;

impl<TMsg> Handler<Reconnect> for ClientActor<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
{
    type Result = ();

    fn handle(&mut self, _msg: Reconnect, ctx: &mut Context<Self>) {
        tracing::info!("Manual reconnection triggered");

        if let Some(hello_actor) = &self.current_hello_actor {
            hello_actor.do_send(zznet_hello::actor::Disconnect);
        }

        self.current_hello_actor = None;
        self.connect_loop(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zznet_session::room_message_trait::RoomMessageTrait;
    use zznet_session::types::RoomId;

    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    enum TestMessages {
        Test(String),
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
            Ok(TestMessages::Test("test".to_string()))
        }

        fn supported_rooms() -> Vec<RoomId> {
            vec![RoomId::from("test")]
        }
    }

    #[test]
    fn test_client_builder_new() {
        let builder = ClientBuilder::<TestMessages>::new();
        assert!(builder.remote_addr.is_none());
        assert_eq!(builder.our_role, AuthRole::Collector);
        assert!(builder.offered_rooms.is_empty());
        assert!(builder.auto_reconnect);
    }

    #[test]
    fn test_client_builder_fluent_api() {
        let builder = ClientBuilder::<TestMessages>::new()
            .connect_to("127.0.0.1:8080")
            .as_role(AuthRole::Database)
            .offer_rooms(vec!["test".to_string()])
            .handshake_timeout(Duration::from_secs(5))
            .reconnect_delay(Duration::from_secs(10))
            .auto_reconnect(false);

        assert_eq!(builder.remote_addr.unwrap(), "127.0.0.1:8080");
        assert_eq!(builder.our_role, AuthRole::Database);
        assert_eq!(builder.offered_rooms, vec!["test".to_string()]);
        assert_eq!(builder.handshake_timeout, Duration::from_secs(5));
        assert_eq!(builder.reconnect_delay, Duration::from_secs(10));
        assert!(!builder.auto_reconnect);
    }

    #[test]
    fn test_validation_missing_remote_addr() {
        let builder = ClientBuilder::<TestMessages>::new().offer_rooms(vec!["test".to_string()]);

        let result = builder.validate();
        assert!(matches!(result, Err(BuilderError::MissingConfig(_))));
    }

    #[test]
    fn test_validation_missing_rooms() {
        let builder = ClientBuilder::<TestMessages>::new().connect_to("127.0.0.1:8080");

        let result = builder.validate();
        assert!(matches!(result, Err(BuilderError::InvalidConfig(_))));
    }

    #[test]
    fn test_validation_success() {
        let builder = ClientBuilder::<TestMessages>::new()
            .connect_to("127.0.0.1:8080")
            .offer_rooms(vec!["test".to_string()]);

        let result = builder.validate();
        assert!(result.is_ok());
    }
}
