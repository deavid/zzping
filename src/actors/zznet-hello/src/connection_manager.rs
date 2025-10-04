//! ConnectionManager Actor - Coordinates HelloActors and SessionManager
//!
//! This actor sits between the transport layer and the session layer:
//! - Spawns HelloActor for each new connection
//! - Receives HandshakeComplete notifications from HelloActors
//! - Creates PeerSession instances in SessionManager
//! - Routes messages between HelloActors and application components

use crate::actor::{HelloActor, HelloConfig, start_hello_actor_with_session_manager};
use crate::session_messages::HandshakeComplete;
use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;
use zznet_api::transport::TransportConnection;
use zznet_session::peer_session::PeerSession;
use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::session_manager::SessionManager;
use zznet_session::types::{PeerId, RoomId};

/// ConnectionManager coordinates HelloActors and SessionManager
///
/// Generic over TMsg: the application's message enum type
pub struct ConnectionManager<TMsg>
where
    TMsg: RoomMessageTrait,
{
    /// Manages all peer sessions
    session_manager: SessionManager<TMsg>,

    /// Maps PeerId to HelloActor address
    /// Used to send InboundRoomMessage to the correct HelloActor
    hello_actors: HashMap<PeerId, Addr<HelloActor>>,
}

impl<TMsg> ConnectionManager<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
{
    /// Create a new ConnectionManager
    ///
    /// `offered_rooms`: Rooms this manager offers to peers
    pub fn new(offered_rooms: Vec<RoomId>) -> Self {
        Self {
            session_manager: SessionManager::new(offered_rooms),
            hello_actors: HashMap::new(),
        }
    }

    /// Add a pre-configured PeerSession to the SessionManager
    ///
    /// This allows the application to create Room<T> instances with
    /// different T types before adding to the session.
    pub fn add_peer(&mut self, peer_id: PeerId, peer_session: PeerSession<TMsg>) {
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

impl<TMsg> Actor for ConnectionManager<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
{
    type Context = Context<Self>;
}

/// Message to get list of connected peer IDs
#[derive(Message)]
#[rtype(result = "Vec<PeerId>")]
pub struct GetPeers;

/// Handler for GetPeers - Return list of connected peer IDs
impl<TMsg> Handler<GetPeers> for ConnectionManager<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
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
    pub peer_id: PeerId,
    _phantom: std::marker::PhantomData<TMsg>,
}

impl<TMsg> GetPeerSender<TMsg> {
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Handler for GetPeerSender - Return cloneable sender for a peer
impl<TMsg> Handler<GetPeerSender<TMsg>> for ConnectionManager<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
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
    pub peer_id: PeerId,
    _phantom: std::marker::PhantomData<TMsg>,
}

impl<TMsg> SubscribePeerInbound<TMsg> {
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Handler for SubscribePeerInbound - Subscribe to inbound messages from peer
impl<TMsg> Handler<SubscribePeerInbound<TMsg>> for ConnectionManager<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
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
    pub peer_id: PeerId,
    pub room_id: RoomId,
    pub message: TMsg,
}

/// Handler for SendToRoom - Forward message to SessionManager
///
/// **Architecture Note**: This is a placeholder that demonstrates the limitation.
/// The real solution is to expose `mpsc::Sender` channels directly to application code
/// instead of routing sends through the actor system.
///
/// See `get_peer_sender()` method below for the proper approach.
impl<TMsg> Handler<SendToRoom<TMsg>> for ConnectionManager<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
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
impl<TMsg> Handler<HandshakeComplete> for ConnectionManager<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
{
    type Result = ();

    fn handle(&mut self, msg: HandshakeComplete, _ctx: &mut Context<Self>) {
        let peer_id = msg.peer_id.clone();
        tracing::info!(
            "Handshake completed - peer_id: {}, peer_role: {:?}, active_rooms: {:?}",
            peer_id,
            msg.peer_role,
            msg.active_rooms
        );

        // 1. Create bidirectional channels for message flow
        let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel(100);
        let (conn_to_session_tx, conn_to_session_rx) = tokio::sync::mpsc::channel(100);
        let (hello_to_conn_tx, hello_to_conn_rx) = tokio::sync::mpsc::channel(100);

        // 2. Create PeerSession for this peer
        let peer_session = PeerSession::new(zznet_session::types::PeerId::from(peer_id.as_str()));

        // Add the peer to SessionManager
        if let Err(e) = self.session_manager.add_peer(
            zznet_session::types::PeerId::from(peer_id.as_str()),
            peer_session,
        ) {
            tracing::error!("Failed to add peer {} to SessionManager: {:?}", peer_id, e);
            return;
        }

        // 3. Connect peer in SessionManager
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

        // 3. Store HelloActor address for future use
        self.hello_actors.insert(
            zznet_session::types::PeerId::from(peer_id.as_str()),
            msg.hello_actor.clone(),
        );

        // 4. Spawn task: SessionManager outbound → HelloActor
        let hello_actor_outbound = msg.hello_actor.clone();
        let peer_id_clone1 = peer_id.clone();
        tokio::spawn(async move {
            while let Some((room_id, message)) = outbound_rx.recv().await {
                // Serialize message using RoomMessageTrait
                let payload = match message.serialize_inner() {
                    Ok(data) => data,
                    Err(e) => {
                        tracing::error!("Failed to serialize message: {:?}", e);
                        continue;
                    }
                };

                // Create SendMessage for HelloActor
                let send_msg = crate::actor::SendMessage {
                    from_room: room_id.as_str().to_string(),
                    to_room: room_id.as_str().to_string(),
                    payload,
                };

                // Send to HelloActor
                if let Err(e) = hello_actor_outbound.send(send_msg).await {
                    tracing::error!("Failed to send message to HelloActor: {:?}", e);
                    break;
                }
            }
            tracing::debug!("Outbound forwarding task for peer {} ended", peer_id_clone1);
        });

        // 5. Give HelloActor the channel for forwarding received messages
        let set_inbound_msg = crate::actor::SetInboundChannel {
            tx: hello_to_conn_tx,
        };
        if let Err(e) = msg.hello_actor.try_send(set_inbound_msg) {
            tracing::error!("Failed to set inbound channel on HelloActor: {:?}", e);
            return;
        }

        // 6. Spawn task: HelloActor inbound → SessionManager
        let peer_id_clone2 = peer_id.clone();
        tokio::spawn(async move {
            let mut hello_to_conn_rx = hello_to_conn_rx;
            while let Some((room_name, payload)) = hello_to_conn_rx.recv().await {
                // Deserialize message using RoomMessageTrait
                let room_id = zznet_session::types::RoomId::from(room_name.as_str());
                match TMsg::deserialize_for_room(&room_id, &payload) {
                    Ok(message) => {
                        // Send to SessionManager
                        if let Err(e) = conn_to_session_tx.try_send((room_id, message)) {
                            tracing::error!(
                                "Failed to send inbound message to SessionManager: {:?}",
                                e
                            );
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to deserialize inbound message: {:?}", e);
                    }
                }
            }
            tracing::debug!("Inbound forwarding task for peer {} ended", peer_id_clone2);
        });

        tracing::info!("Successfully wired channels for peer {}", peer_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zznet_session::types::RoomId;

    // Simple test message enum for ConnectionManager tests
    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    enum TestMessages {
        IntentConfig(String),
        MemDB(String),
        Health(String),
    }

    impl zznet_session::room_message_trait::RoomMessageTrait for TestMessages {
        fn room_id(&self) -> RoomId {
            match self {
                TestMessages::IntentConfig(_) => RoomId::from("intentconfig"),
                TestMessages::MemDB(_) => RoomId::from("memdb"),
                TestMessages::Health(_) => RoomId::from("health"),
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
            Ok(TestMessages::IntentConfig("test".to_string())) // Stub for testing
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

        let _manager = ConnectionManager::<TestMessages>::new(rooms);
        // Just test it compiles and constructs
    }
}
