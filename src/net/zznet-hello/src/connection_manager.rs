//! ConnectionManager Actor - Coordinates HelloActors and SessionManager
//!
//! This actor sits between the transport layer and the session layer:
//! - Spawns HelloActor for each new connection
//! - Receives HandshakeComplete notifications from HelloActors
//! - Creates PeerSession instances in SessionManager
//! - Routes messages between HelloActors and application components

use crate::actor::{HelloActor, HelloConfig, start_hello_actor_with_session_manager};
use crate::session_bridge::SessionBridge;
use crate::session_messages::HandshakeComplete;
use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;
use zznet_api::transport::TransportConnection;
use zznet_auth::ApplicationRole;
use zznet_session::peer_session::PeerSession;
use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::session_manager::SessionManager;
use zznet_session::types::{PeerId, RoomId};

type Authorizer<TRole> =
    Box<dyn Fn(&zznet_api::types::PeerIdentity) -> Option<TRole> + Send + Sync>;

/// ConnectionManager coordinates HelloActors and SessionManager
///
/// SECURITY: ConnectionManager REQUIRES an authorizer function.
/// There is no code path that allows connections without authorization.
/// Every peer must be explicitly authorized before gaining access.
///
/// Generic over TMsg: the application's message enum type
/// Generic over TRole: the application's role type
pub struct ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    /// Manages all peer sessions
    session_manager: SessionManager<TMsg, TRole>,

    /// Maps PeerId to HelloActor address
    /// Used to send InboundRoomMessage to the correct HelloActor
    hello_actors: HashMap<PeerId, Addr<HelloActor>>,
    /// REQUIRED: Authorizer function to resolve peer identity to role.
    /// This is NOT optional - every connection must be authorized.
    /// Takes PeerIdentity (from TLS certificate) and returns role if allowed.
    authorizer: Authorizer<TRole>,
    /// Flag for insecure mode (trusts HELLO claims when TLS unavailable).
    /// In normal operation, should be false (always verify via TLS cert).
    /// Currently unused as we require TLS, but kept for future insecure mode support.
    #[allow(dead_code)]
    insecure_trust_hello: bool,
}

impl<TMsg, TRole> ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    /// Create a new ConnectionManager with a REQUIRED authorizer function.
    ///
    /// SECURITY: An authorizer is MANDATORY.
    /// There is no code path that allows connections without authorization.
    ///
    /// # Arguments
    /// - `offered_rooms`: Rooms this manager offers to peers
    /// - `authorizer`: Function that maps PeerIdentity → Option<Role>
    ///   Returns Some(role) if authorized, None to reject
    pub fn new(offered_rooms: Vec<RoomId>, authorizer: Authorizer<TRole>) -> Self {
        Self {
            session_manager: SessionManager::new_with_limits(offered_rooms, None, None),
            hello_actors: HashMap::new(),
            authorizer,
            insecure_trust_hello: false,
        }
    }

    /// Create a new ConnectionManager with explicit connection limits.
    ///
    /// SECURITY: An authorizer is MANDATORY.
    ///
    /// # Arguments
    /// - `offered_rooms`: Rooms this manager offers to peers
    /// - `max_peers`: Maximum concurrent peer connections
    /// - `max_rooms_per_peer`: Maximum rooms per peer
    /// - `authorizer`: Function that maps PeerIdentity → Option<Role>
    pub fn new_with_limits(
        offered_rooms: Vec<RoomId>,
        max_peers: Option<usize>,
        max_rooms_per_peer: Option<usize>,
        authorizer: Authorizer<TRole>,
    ) -> Self {
        Self {
            session_manager: SessionManager::new_with_limits(
                offered_rooms,
                max_peers,
                max_rooms_per_peer,
            ),
            hello_actors: HashMap::new(),
            authorizer,
            insecure_trust_hello: false,
        }
    }

    /// Create a new ConnectionManager with optional insecure trust mode.
    ///
    /// SECURITY: An authorizer is MANDATORY.
    ///
    /// # Arguments
    /// - `offered_rooms`: Rooms this manager offers to peers
    /// - `authorizer`: Function that maps PeerIdentity → Option<Role>
    /// - `insecure_trust_hello`: If true, trust HELLO messages in non-TLS mode.
    ///   Default should be false (always require TLS/certificate verification).
    pub fn new_with_insecure_trust(
        offered_rooms: Vec<RoomId>,
        authorizer: Authorizer<TRole>,
        insecure_trust_hello: bool,
    ) -> Self {
        Self {
            session_manager: SessionManager::new_with_limits(offered_rooms, None, None),
            hello_actors: HashMap::new(),
            authorizer,
            insecure_trust_hello,
        }
    }

    /// Create a new ConnectionManager with all options.
    ///
    /// SECURITY: An authorizer is MANDATORY.
    ///
    /// # Arguments
    /// - `offered_rooms`: Rooms this manager offers to peers
    /// - `max_peers`: Maximum concurrent peer connections
    /// - `max_rooms_per_peer`: Maximum rooms per peer
    /// - `authorizer`: Function that maps PeerIdentity → Option<Role>
    /// - `insecure_trust_hello`: If true, trust HELLO messages in non-TLS mode.
    pub fn new_with_limits_and_insecure(
        offered_rooms: Vec<RoomId>,
        max_peers: Option<usize>,
        max_rooms_per_peer: Option<usize>,
        authorizer: Authorizer<TRole>,
        insecure_trust_hello: bool,
    ) -> Self {
        Self {
            session_manager: SessionManager::new_with_limits(
                offered_rooms,
                max_peers,
                max_rooms_per_peer,
            ),
            hello_actors: HashMap::new(),
            authorizer,
            insecure_trust_hello,
        }
    }

    /// Add a pre-configured PeerSession to the SessionManager
    ///
    /// This allows the application to create Room<T> instances with
    /// different T types before adding to the session.
    pub fn add_peer(&mut self, peer_id: PeerId, peer_session: PeerSession<TMsg, TRole>) {
        if let Err(e) = self.session_manager.add_peer(peer_id.clone(), peer_session) {
            tracing::error!("Failed to add peer {}: {:?}", peer_id, e);
        }
    }

    /// Spawn a new HelloActor for an incoming/outgoing connection
    ///
    /// `peer_id`: Identifier for this peer
    /// `transport`: The transport for this connection
    /// `config`: HelloConfig for this connection
    pub fn spawn_hello_actor(
        &mut self,
        peer_id: PeerId,
        transport: Box<dyn TransportConnection>,
        config: HelloConfig,
        ctx: &mut Context<Self>,
    ) -> Addr<HelloActor> {
        // Use the public API to start HelloActor with SessionManager integration
        let addr = start_hello_actor_with_session_manager(
            transport,
            config,
            Some(ctx.address().recipient()),
        );

        self.hello_actors.insert(peer_id, addr.clone());
        addr
    }

    /// Get a cloneable sender for a specific peer
    ///
    /// This is the proper way for application code to send messages to peers.
    /// The returned sender can be cloned and used from any async context without
    /// needing to go through the actor system.
    ///
    ///
    ///
    /// Returns `None` if peer doesn't exist or isn't connected.
    pub fn get_peer_sender(&self, peer_id: &PeerId) -> Option<mpsc::Sender<(RoomId, TMsg)>> {
        self.session_manager.get_peer_sender(peer_id)
    }

    /// Subscribe to inbound messages from a specific peer
    ///
    /// Returns a broadcast receiver that will receive all inbound messages from the peer.
    /// This is useful for clients that want to handle messages directly without using
    /// the Room abstraction (e.g., request-response patterns).
    ///
    /// Multiple subscribers can call this method to get independent receivers.
    ///
    ///
    ///
    /// Returns `None` if peer doesn't exist or isn't connected.
    pub fn subscribe_peer_inbound(
        &mut self,
        peer_id: &PeerId,
    ) -> Option<tokio::sync::broadcast::Receiver<(RoomId, TMsg)>> {
        self.session_manager.subscribe_peer_inbound(peer_id)
    }

    /// Get list of connected peer IDs
    pub fn peer_ids(&self) -> Vec<PeerId> {
        self.session_manager.peer_ids()
    }
}

impl<TMsg, TRole> Actor for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    type Context = Context<Self>;
}

/// Message for handing a transport connection to the ConnectionManager.
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct HandleTransport {
    /// The transport connection to manage (boxed trait object).
    pub transport: Box<dyn zznet_api::transport::TransportConnection>,
    /// HelloActor configuration for this connection.
    pub config: crate::actor::HelloConfig,
}

impl<TMsg, TRole> Handler<HandleTransport> for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    type Result = Result<(), String>;

    fn handle(&mut self, msg: HandleTransport, ctx: &mut Context<Self>) -> Self::Result {
        // Use peer_addr string as a temporary peer id until handshake provides canonical id
        let peer_identity = msg.transport.peer_identity();
        let peer_id = PeerId::from(peer_identity.peer_addr.as_str());

        // Spawn HelloActor managed by this ConnectionManager (it will wire to SessionManager)
        let _addr = self.spawn_hello_actor(peer_id, msg.transport, msg.config, ctx);
        Ok(())
    }
}

/// Message to get list of connected peer IDs
#[derive(Message)]
#[rtype(result = "Vec<PeerId>")]
pub struct GetPeers;

/// Handler for GetPeers - Return list of connected peer IDs
impl<TMsg, TRole> Handler<GetPeers> for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    type Result = Vec<PeerId>;

    fn handle(&mut self, _msg: GetPeers, _ctx: &mut Context<Self>) -> Self::Result {
        self.peer_ids()
    }
}

/// Message to get a cloneable sender for a peer
#[derive(Message)]
#[rtype(result = "Option<mpsc::Sender<(RoomId, TMsg)>>")]
pub struct GetPeerSender<TMsg: 'static> {
    /// The peer to query for a cloneable sender.
    pub peer_id: PeerId,
    _phantom: std::marker::PhantomData<TMsg>,
}

impl<TMsg> GetPeerSender<TMsg> {
    /// Create a `GetPeerSender` message for the given peer id.
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Handler for GetPeerSender - Return cloneable sender for a peer
impl<TMsg, TRole> Handler<GetPeerSender<TMsg>> for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    type Result = Option<mpsc::Sender<(RoomId, TMsg)>>;

    fn handle(&mut self, msg: GetPeerSender<TMsg>, _ctx: &mut Context<Self>) -> Self::Result {
        self.get_peer_sender(&msg.peer_id)
    }
}

/// Message to subscribe to inbound messages from a peer
#[derive(Message)]
#[rtype(result = "Option<tokio::sync::broadcast::Receiver<(RoomId, TMsg)>>")]
pub struct SubscribePeerInbound<TMsg: 'static> {
    /// The peer to subscribe to inbound messages from.
    pub peer_id: PeerId,
    _phantom: std::marker::PhantomData<TMsg>,
}

impl<TMsg> SubscribePeerInbound<TMsg> {
    /// Create a `SubscribePeerInbound` message for the given peer id.
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Handler for SubscribePeerInbound - Subscribe to inbound messages from peer
impl<TMsg, TRole> Handler<SubscribePeerInbound<TMsg>> for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    type Result = Option<tokio::sync::broadcast::Receiver<(RoomId, TMsg)>>;

    fn handle(
        &mut self,
        msg: SubscribePeerInbound<TMsg>,
        _ctx: &mut Context<Self>,
    ) -> Self::Result {
        self.subscribe_peer_inbound(&msg.peer_id)
    }
}

/// Message to send a typed message to a peer's room
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct SendToRoom<TMsg> {
    /// Target peer id.
    pub peer_id: PeerId,
    /// Target room id within the peer.
    pub room_id: RoomId,
    /// The typed message to send.
    pub message: TMsg,
}

/// Handler for SendToRoom - Forward message to SessionManager
///
/// **Architecture Note**: This is a placeholder that demonstrates the limitation.
/// The real solution is to expose `mpsc::Sender` channels directly to application code
/// instead of routing sends through the actor system.
///
/// See `get_peer_sender()` method below for the proper approach.
impl<TMsg, TRole> Handler<SendToRoom<TMsg>> for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, _msg: SendToRoom<TMsg>, _ctx: &mut Context<Self>) -> Self::Result {
        // We can't access session_manager methods in async context without Arc
        // The real solution: don't use this pattern! Use get_peer_sender() instead.
        let error = "SendToRoom through actor messages not supported. \
             Use ConnectionManager::get_peer_sender() to get a cloneable sender handle.";
        Box::pin(async move { Err(error.to_string()) })
    }
}

/// Handler for HandshakeComplete - Called when HelloActor completes handshake
///
/// SECURITY: Every connection goes through the mandatory authorizer.
/// No code path allows unauthenticated connections.
impl<TMsg, TRole> Handler<HandshakeComplete> for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    type Result = ();

    fn handle(&mut self, msg: HandshakeComplete, _ctx: &mut Context<Self>) {
        let peer_id = msg.peer_id.clone();
        tracing::info!(
            "Handshake completed - peer_id: {}, peer_role_from_hello: {}, active_rooms: {:?}",
            peer_id,
            msg.peer_role_str,
            msg.active_rooms
        );

        // SECURITY: Authorizer is MANDATORY. Resolve peer role from PeerIdentity (TLS cert).
        // peer_role_str from HELLO is logged but not used for authorization.
        // Authorization decisions are based on cryptographically verified TLS certificate only.
        match (self.authorizer)(&msg.peer_identity) {
            Some(role) => {
                tracing::info!(
                    "Peer {} ({}) authorized as {:?} (identity: {})",
                    peer_id,
                    msg.peer_role_str,
                    role,
                    msg.peer_identity.full_identity()
                );

                // Create PeerSession for this peer and set auth context
                let mut peer_session =
                    PeerSession::new(zznet_session::types::PeerId::from(peer_id.as_str()));
                peer_session.set_role(Some(role));
                peer_session.set_identity(msg.peer_identity.clone());

                // Add the peer to SessionManager
                if let Err(e) = self.session_manager.add_peer(
                    zznet_session::types::PeerId::from(peer_id.as_str()),
                    peer_session,
                ) {
                    tracing::error!("Failed to add peer {} to SessionManager: {:?}", peer_id, e);
                    return;
                }

                // Create channels for SessionBridge
                let (outbound_tx, outbound_rx) = tokio::sync::mpsc::channel(100);
                let (conn_to_session_tx, conn_to_session_rx) = tokio::sync::mpsc::channel(100);
                let (hello_to_conn_tx, hello_to_conn_rx) = tokio::sync::mpsc::channel(100);

                // Connect peer in SessionManager
                if let Err(e) = self.session_manager.connect_peer(
                    zznet_session::types::PeerId::from(peer_id.as_str()),
                    outbound_tx,
                    conn_to_session_rx,
                ) {
                    tracing::error!(
                        "Failed to connect peer {} in SessionManager: {:?}",
                        peer_id,
                        e
                    );
                    return;
                }

                // Store HelloActor address for future use
                self.hello_actors.insert(
                    zznet_session::types::PeerId::from(peer_id.as_str()),
                    msg.hello_actor.clone(),
                );

                // Give HelloActor the channel for forwarding received messages
                let set_inbound_msg = crate::actor::SetInboundChannel {
                    tx: hello_to_conn_tx,
                };
                if let Err(e) = msg.hello_actor.try_send(set_inbound_msg) {
                    tracing::error!("Failed to set inbound channel on HelloActor: {:?}", e);
                    return;
                }

                // Spawn SessionBridge actor to handle all the complex wiring
                let bridge_peer_id = peer_id.clone();
                let bridge = SessionBridge::new(
                    bridge_peer_id,
                    msg.hello_actor.clone(),
                    outbound_rx,
                    conn_to_session_tx,
                    hello_to_conn_rx,
                );

                // Start the SessionBridge in the actor system
                let _bridge_addr = bridge.start();

                tracing::info!(
                    "Successfully authorized and started SessionBridge for peer {} - message forwarding active",
                    peer_id
                );
            }
            None => {
                // SECURITY: Authorization FAILED. Loud logging and immediate disconnect.
                tracing::error!(
                    "!!! SECURITY REJECTION !!!: Peer {} REJECTED by authorizer",
                    peer_id
                );
                tracing::error!(
                    "    Identity: {} | HELLO claimed role: {}",
                    msg.peer_identity.full_identity(),
                    msg.peer_role_str
                );
                tracing::warn!("Disconnecting unauthorized peer {}", peer_id);
                msg.hello_actor.do_send(crate::actor::Disconnect);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zznet_session::types::RoomId;

    // Simple test message enum for ConnectionManager tests
    #[derive(Debug, Clone)]
    enum TestMessages {
        IntentConfig,
        MemDB,
        Health,
    }

    impl zznet_session::room_message_trait::RoomMessageTrait for TestMessages {
        fn room_id(&self) -> RoomId {
            match self {
                TestMessages::IntentConfig => RoomId::from("intentconfig"),
                TestMessages::MemDB => RoomId::from("memdb"),
                TestMessages::Health => RoomId::from("health"),
            }
        }

        fn serialize_inner(
            &self,
        ) -> Result<Vec<u8>, zznet_session::room_message_trait::SerializationError> {
            Ok(vec![]) // Stub for testing
        }

        fn deserialize_for_room(
            _room_id: &RoomId,
            _bytes: &[u8],
        ) -> Result<Self, zznet_session::room_message_trait::DeserializationError> {
            Ok(TestMessages::IntentConfig) // Stub for testing
        }

        fn supported_rooms() -> Vec<RoomId> {
            vec![
                RoomId::from("intentconfig"),
                RoomId::from("memdb"),
                RoomId::from("health"),
            ]
        }
    }

    #[actix::test]
    async fn test_connection_manager_creation() {
        let rooms = vec![
            RoomId::from("intentconfig"),
            RoomId::from("memdb"),
            RoomId::from("health"),
        ];
        // Construct each variant to satisfy dead-code checks for tests.
        let _a = TestMessages::IntentConfig;
        let _b = TestMessages::MemDB;
        let _c = TestMessages::Health;

        // Create a mock authorizer that accepts all peers as MockRole::Admin
        let authorizer: Authorizer<zznet_auth::mock::MockRole> =
            Box::new(|_peer_identity| Some(zznet_auth::mock::MockRole::Admin));

        let _manager =
            ConnectionManager::<TestMessages, zznet_auth::mock::MockRole>::new(rooms, authorizer);
        // Just test it compiles and constructs
    }
}
