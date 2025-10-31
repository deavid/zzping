//! ConnectionManager Actor - Coordinates HelloActors and SessionManager
//!
//! This actor sits between the transport layer and the session layer:
//! - Spawns HelloActor for each new connection
//! - Receives HandshakeComplete notifications from HelloActors
//! - Registers peer state and channel sets with the session layer
//! - Routes messages between HelloActors and application components
//! - Wires registered room handlers for new peers

use crate::actor::{HelloActor, HelloConfig, start_hello_actor_with_session_manager};
use crate::session_bridge::SessionBridge;
use crate::session_messages::HandshakeComplete;
use actix::prelude::*;
use std::collections::{HashMap, HashSet};
use tokio::sync::{broadcast, mpsc};
use zznet_api::transport::TransportConnection;
use zznet_api::types::Role;
use zznet_api::types::{PeerId, RoomId};
use zznet_router::{PeerSender, RouterActor, SubscribePeerInbound as RouterSubscribePeerInbound};

// Authorizer type removed - authorization is represented by a set of allowed Roles

/// ConnectionManager coordinates HelloActors and PeerManager
///
/// SECURITY: ConnectionManager REQUIRES an authorizer function.
/// There is no code path that allows connections without authorization.
/// Every peer must be explicitly authorized before gaining access.
///
/// Generic over TRole: the application's role type
pub struct ConnectionManager {
    /// RouterActor address for data-plane operations
    ///
    /// DESIGN: RouterActor handles peer channels and message routing
    /// - ConnectionManager sends OnPeerConnected directly to RouterActor after HELLO handshake completes
    /// - RouterActor manages peer sessions and routing
    router_actor: Addr<RouterActor>,

    /// Maps PeerId to HelloActor address
    /// Used to send InboundRoomMessage to the correct HelloActor
    hello_actors: HashMap<PeerId, Addr<HelloActor>>,
    /// Set of allowed canonical `Role`s for this ConnectionManager.
    /// Connections will only be accepted when the HELLO role string maps to
    /// a `Role` that appears in this set.
    allowed_roles: HashSet<Role>,
}

impl ConnectionManager {
    /// Create a new ConnectionManager with RouterActor.
    ///
    /// SECURITY: A set of allowed roles is REQUIRED. There is no code path that
    /// allows connections without explicit allowed roles.
    ///
    /// # Arguments
    /// - `router_actor`: RouterActor address for data-plane operations
    /// - `allowed_roles`: set of canonical `zznet_api::types::Role` strings that are permitted
    pub fn new(router_actor: Addr<RouterActor>, allowed_roles: HashSet<Role>) -> Self {
        Self {
            router_actor,
            hello_actors: HashMap::new(),
            allowed_roles,
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
}

impl Actor for ConnectionManager {
    type Context = Context<Self>;
}

/// Internal message used to notify ConnectionManager actor that async post-processing
/// after handshake is complete and bridge started. This allows the async task to
/// perform SessionManager mutations and start the SessionBridge, then inform the actor
/// to update actor-local state without blocking.
#[derive(Message)]
#[rtype(result = "()")]
struct HandshakePostProcessed {
    peer_id: zznet_api::types::PeerId,
    hello_actor: Addr<HelloActor>,
}

impl Handler<HandshakePostProcessed> for ConnectionManager {
    type Result = ();

    fn handle(&mut self, msg: HandshakePostProcessed, _ctx: &mut Context<Self>) {
        self.hello_actors.insert(msg.peer_id, msg.hello_actor);
    }
}

/// Internal message carrying the channels to start a SessionBridge inside the actor context.
#[derive(Message)]
#[rtype(result = "()")]
struct HandshakePostProcessedInner {
    peer_id: zznet_api::types::PeerId,
    hello_actor: Addr<HelloActor>,
    outbound_rx: Option<tokio::sync::mpsc::Receiver<(RoomId, Vec<u8>)>>,
    conn_to_session_tx: tokio::sync::mpsc::Sender<(RoomId, Vec<u8>)>,
    hello_to_conn_rx: Option<tokio::sync::mpsc::Receiver<(String, Vec<u8>)>>,
}

impl Handler<HandshakePostProcessedInner> for ConnectionManager {
    type Result = ();

    fn handle(&mut self, msg: HandshakePostProcessedInner, _ctx: &mut Context<Self>) {
        // Store HelloActor address for future use
        self.hello_actors
            .insert(msg.peer_id.clone(), msg.hello_actor.clone());

        // Start SessionBridge in the actor context so it can call spawn_local if needed.
        if let (Some(outbound_rx), Some(hello_to_conn_rx)) = (msg.outbound_rx, msg.hello_to_conn_rx)
        {
            let bridge = SessionBridge::new(
                msg.peer_id.as_str().to_string(),
                msg.hello_actor.clone(),
                outbound_rx,
                msg.conn_to_session_tx,
                hello_to_conn_rx,
            );
            let _addr = bridge.start();
        } else {
            tracing::warn!(
                "HandshakePostProcessedInner missing channels for peer {:?}",
                msg.peer_id
            );
        }
    }
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

impl Handler<HandleTransport> for ConnectionManager {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: HandleTransport, ctx: &mut Context<Self>) -> Self::Result {
        // Use peer_addr string as a temporary peer id until handshake provides canonical id
        let peer_addr = msg
            .transport
            .peer_addr()
            .unwrap_or_else(|| "unknown".to_string());
        let peer_id = PeerId::from(peer_addr.as_str());

        // Spawn HelloActor managed by this ConnectionManager (it will wire to SessionManager)
        let _addr = self.spawn_hello_actor(peer_id, msg.transport, msg.config, ctx);
        Ok(())
    }
}

/// Message to get list of connected peer IDs
#[derive(Message)]
#[rtype(result = "Vec<PeerId>")]
pub struct GetPeers;

/// Handler for GetPeers - Returns list of connected HelloActor peer IDs
impl Handler<GetPeers> for ConnectionManager {
    type Result = Vec<PeerId>;

    fn handle(&mut self, _msg: GetPeers, _ctx: &mut Context<Self>) -> Self::Result {
        self.hello_actors.keys().cloned().collect()
    }
}

/// Message to get a cloneable sender for a peer
#[derive(Message)]
#[rtype(result = "Option<mpsc::Sender<(RoomId, Vec<u8>)>>")]
pub struct GetPeerSender {
    /// The peer to query for a cloneable sender.
    pub peer_id: PeerId,
}

impl GetPeerSender {
    /// Create a `GetPeerSender` message for the given peer id.
    pub fn new(peer_id: PeerId) -> Self {
        Self { peer_id }
    }
}

/// Handler for GetPeerSender - Forward to PeerManagerActor via message passing
impl Handler<GetPeerSender> for ConnectionManager {
    type Result = ResponseFuture<Option<mpsc::Sender<(RoomId, Vec<u8>)>>>;

    fn handle(&mut self, msg: GetPeerSender, _ctx: &mut Context<Self>) -> Self::Result {
        let router_addr = self.router_actor.clone();
        Box::pin(async move {
            router_addr
                .send(PeerSender {
                    peer_id: msg.peer_id,
                })
                .await
                .ok()
                .flatten()
        })
    }
}

/// Message to subscribe to inbound messages from a peer
#[derive(Message)]
#[rtype(result = "Option<tokio::sync::broadcast::Receiver<(RoomId, Vec<u8>)>>")]
pub struct SubscribePeerInbound {
    /// The peer to subscribe to inbound messages from.
    pub peer_id: PeerId,
}

impl SubscribePeerInbound {
    /// Create a `SubscribePeerInbound` message for the given peer id.
    pub fn new(peer_id: PeerId) -> Self {
        Self { peer_id }
    }
}

/// Handler for SubscribePeerInbound - Forward to PeerManagerActor via message passing
impl Handler<SubscribePeerInbound> for ConnectionManager {
    type Result = ResponseFuture<Option<broadcast::Receiver<(RoomId, Vec<u8>)>>>;

    fn handle(&mut self, msg: SubscribePeerInbound, _ctx: &mut Context<Self>) -> Self::Result {
        let router_addr = self.router_actor.clone();
        Box::pin(async move {
            router_addr
                .send(RouterSubscribePeerInbound {
                    peer_id: msg.peer_id,
                })
                .await
                .ok()
                .flatten()
        })
    }
}

/// Message to send serialized data to a peer's room
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct SendToRoom {
    /// Target peer id.
    pub peer_id: PeerId,
    /// Target room id within the peer.
    pub room_id: RoomId,
    /// The serialized message data.
    pub data: Vec<u8>,
}

/// Handler for SendToRoom - Forward message to SessionManager
///
/// **Architecture Note**: This is a placeholder that demonstrates the limitation.
/// The real solution is to expose `mpsc::Sender` channels directly to application code
/// instead of routing sends through the actor system.
///
/// See `get_peer_sender()` method below for the proper approach.
impl Handler<SendToRoom> for ConnectionManager {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, _msg: SendToRoom, _ctx: &mut Context<Self>) -> Self::Result {
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
impl Handler<HandshakeComplete> for ConnectionManager {
    type Result = ();

    fn handle(&mut self, msg: HandshakeComplete, _ctx: &mut Context<Self>) {
        let peer_id = msg.peer_id.clone();
        tracing::info!(
            "Handshake completed - peer_id: {}, peer_role_from_hello: {}, active_rooms: {:?}",
            peer_id,
            msg.peer_role_str,
            msg.active_rooms
        );

        // SECURITY: Authorizer is MANDATORY.
        // HELLO role is the PRIMARY source (no TLS validation needed).
        let role = Role::new(&msg.peer_role_str);

        // Check if role is allowed
        if !self.allowed_roles.contains(&role) {
            // SECURITY: Authorization FAILED. Loud logging and immediate disconnect.
            tracing::error!(
                "!!! SECURITY REJECTION !!!: Peer {} REJECTED by authorizer",
                peer_id
            );
            tracing::error!(
                "    HELLO claimed role: {} (not in allowed roles)",
                msg.peer_role_str
            );
            tracing::warn!("Disconnecting unauthorized peer {}", peer_id);
            msg.hello_actor.do_send(crate::actor::Disconnect);
            return;
        }

        tracing::info!(
            "Peer {} authorized as {:?} (HELLO={})",
            peer_id,
            role,
            msg.peer_role_str
        );

        // Prepare data to send connection directly to RouterActor
        let router_addr = self.router_actor.clone();
        let hello_actor = msg.hello_actor.clone();
        let actor_addr = _ctx.address();

        // Create channels for SessionBridge
        let (outbound_tx, outbound_rx) = tokio::sync::mpsc::channel(100);
        let (conn_to_session_tx, conn_to_session_rx) = tokio::sync::mpsc::channel(100);
        let (hello_to_conn_tx, hello_to_conn_rx) = tokio::sync::mpsc::channel(100);

        // Spawn an async task to connect peer directly to RouterActor
        tokio::spawn(async move {
            let peer_id_api = zznet_api::types::PeerId::from(peer_id.as_str());

            // Send OnPeerConnected directly to RouterActor with Role
            let connect_result = router_addr
                .send(zznet_router::OnPeerConnected {
                    peer_id: peer_id_api.clone(),
                    role: role.clone(),
                    outbound_tx: outbound_tx.clone(),
                    inbound_rx: conn_to_session_rx,
                })
                .await;

            if let Err(e) = connect_result {
                tracing::error!(
                    "Failed to connect peer to RouterActor for {}: {:?}",
                    peer_id,
                    e
                );
                return;
            }

            match connect_result.unwrap() {
                Ok(_) => {
                    tracing::info!("Successfully connected peer {} to RouterActor", peer_id);
                }
                Err(error_msg) => {
                    tracing::error!("RouterActor rejected peer {}: {}", peer_id, error_msg);
                    return;
                }
            }

            // Give HelloActor the channel for forwarding received messages
            let set_inbound_msg = crate::actor::SetInboundChannel {
                tx: hello_to_conn_tx,
            };
            if let Err(e) = hello_actor.try_send(set_inbound_msg) {
                tracing::error!("Failed to set inbound channel on HelloActor: {:?}", e);
                // continue - we still notify the actor to start the bridge
            }

            // Prepare HelloActor clone to send back to actor
            let send_hello_actor = hello_actor.clone();

            // Send channels back to actor so it can start the SessionBridge inside
            // the actor context (this avoids spawn_local being called outside LocalSet).
            let inner_msg = HandshakePostProcessedInner {
                peer_id: peer_id_api.clone(),
                hello_actor: send_hello_actor,
                outbound_rx: Some(outbound_rx),
                conn_to_session_tx: conn_to_session_tx.clone(),
                hello_to_conn_rx: Some(hello_to_conn_rx),
            };

            tracing::info!("Connected to peer {} as {:?}", peer_id, role);

            actor_addr.do_send(inner_msg);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zznet_api::types::RoomId;

    // Simple test message enum for ConnectionManager tests
    #[derive(Debug, Clone)]
    enum TestMessages {
        IntentConfig,
        MemDB,
        Health,
    }

    impl zznet_room::room_message_trait::RoomMessageTrait for TestMessages {
        fn room_id(&self) -> RoomId {
            match self {
                TestMessages::IntentConfig => RoomId::from("intentconfig"),
                TestMessages::MemDB => RoomId::from("memdb"),
                TestMessages::Health => RoomId::from("health"),
            }
        }

        fn serialize_inner(
            &self,
        ) -> Result<Vec<u8>, zznet_room::room_message_trait::SerializationError> {
            Ok(vec![]) // Stub for testing
        }

        fn deserialize_for_room(
            _room_id: &RoomId,
            _bytes: &[u8],
        ) -> Result<Self, zznet_room::room_message_trait::DeserializationError> {
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
        // Construct each variant to satisfy dead-code checks for tests.
        let _a = TestMessages::IntentConfig;
        let _b = TestMessages::MemDB;
        let _c = TestMessages::Health;

        // Create RouterActor
        let router_actor = zznet_router::RouterActor::new(vec![], None).start();

        // Build allowed roles set for test (accept any admin role)
        let mut allowed = HashSet::new();
        allowed.insert(Role::new("admin"));

        let _manager = ConnectionManager::new(router_actor, allowed);
        // Just test it compiles and constructs
    }
}
