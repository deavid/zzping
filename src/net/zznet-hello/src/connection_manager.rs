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
    /// Optional ACL manager and insecure_trust flag.
    /// Optional authorizer: takes PeerIdentity and returns resolved AuthRole if allowed
    acl: Option<(Authorizer<TRole>, bool)>,
}

impl<TMsg, TRole> ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait + 'static,
    TRole: ApplicationRole,
{
    /// Create a new ConnectionManager
    ///
    /// `offered_rooms`: Rooms this manager offers to peers
    pub fn new(offered_rooms: Vec<RoomId>) -> Self {
        Self {
            session_manager: SessionManager::new_with_limits(offered_rooms, None, None),
            hello_actors: HashMap::new(),
            acl: None,
        }
    }

    /// Create a new ConnectionManager with explicit connection limits
    pub fn new_with_limits(
        offered_rooms: Vec<RoomId>,
        max_peers: Option<usize>,
        max_rooms_per_peer: Option<usize>,
    ) -> Self {
        Self {
            session_manager: SessionManager::new_with_limits(
                offered_rooms,
                max_peers,
                max_rooms_per_peer,
            ),
            hello_actors: HashMap::new(),
            acl: None,
        }
    }

    /// Create a ConnectionManager with an optional AclManager and insecure_trust flag
    pub fn new_with_acl(
        offered_rooms: Vec<RoomId>,
        acl: Option<(Authorizer<TRole>, bool)>,
    ) -> Self {
        Self {
            session_manager: SessionManager::new_with_limits(offered_rooms, None, None),
            hello_actors: HashMap::new(),
            acl,
        }
    }

    /// Create a ConnectionManager with ACL and explicit limits
    pub fn new_with_limits_and_acl(
        offered_rooms: Vec<RoomId>,
        acl: Option<(Authorizer<TRole>, bool)>,
        max_peers: Option<usize>,
        max_rooms_per_peer: Option<usize>,
    ) -> Self {
        Self {
            session_manager: SessionManager::new_with_limits(
                offered_rooms,
                max_peers,
                max_rooms_per_peer,
            ),
            hello_actors: HashMap::new(),
            acl,
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
    TMsg: RoomMessageTrait + 'static,
    TRole: ApplicationRole,
{
    type Context = Context<Self>;
}

/// Message to get list of connected peer IDs
#[derive(Message)]
#[rtype(result = "Vec<PeerId>")]
pub struct GetPeers;

/// Handler for GetPeers - Return list of connected peer IDs
impl<TMsg, TRole> Handler<GetPeers> for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait + 'static,
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
    TMsg: RoomMessageTrait + 'static,
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
    TMsg: RoomMessageTrait + 'static,
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
    TMsg: RoomMessageTrait + 'static,
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
/// Simplified: Delegates complex message wiring to SessionBridge actor.
impl<TMsg, TRole> Handler<HandshakeComplete> for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait + 'static,
    TRole: ApplicationRole,
{
    type Result = ();

    fn handle(&mut self, msg: HandshakeComplete, _ctx: &mut Context<Self>) {
        let peer_id = msg.peer_id.clone();
        tracing::info!(
            "Handshake completed - peer_id: {}, peer_role: {}, active_rooms: {:?}",
            peer_id,
            msg.peer_role_str,
            msg.active_rooms
        );

        // Resolve the peer's role using the ACL authorizer
        let resolved_role = if let Some((authorizer, _insecure_flag)) = &self.acl {
            match authorizer(&msg.peer_identity) {
                Some(role) => {
                    tracing::info!(
                        "Peer {} authorized by ACL as {:?} (identity: {})",
                        peer_id,
                        role,
                        msg.peer_identity.full_identity()
                    );
                    Some(role)
                }
                None => {
                    tracing::warn!(
                        "Peer {} denied by ACL - disconnecting (identity: {})",
                        peer_id,
                        msg.peer_identity.full_identity()
                    );
                    msg.hello_actor.do_send(crate::actor::Disconnect);
                    return;
                }
            }
        } else {
            // No ACL configured - allow connection but no role
            tracing::debug!("Peer {} connected without ACL (no role assigned)", peer_id);
            None
        };

        // Create PeerSession for this peer and set auth context
        let mut peer_session =
            PeerSession::new(zznet_session::types::PeerId::from(peer_id.as_str()));
        peer_session.set_role(resolved_role);
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
        // This replaces the two manual tokio::spawn tasks that were previously here
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
            "Successfully started SessionBridge for peer {} - message forwarding active",
            peer_id
        );
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

        let _manager = ConnectionManager::<TestMessages, zznet_auth::mock::MockRole>::new(rooms);
        // Just test it compiles and constructs
    }
}
