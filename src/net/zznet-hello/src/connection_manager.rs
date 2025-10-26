//! ConnectionManager Actor - Coordinates HelloActors and SessionManager
//!
//! This actor sits between the transport layer and the session layer:
//! - Spawns HelloActor for each new connection
//! - Receives HandshakeComplete notifications from HelloActors
//! - Creates PeerSession instances in SessionManager
//! - Routes messages between HelloActors and application components
//! - Wires registered room handlers for new peers

use crate::actor::{HelloActor, HelloConfig, start_hello_actor_with_session_manager};
use crate::session_bridge::SessionBridge;
use crate::session_messages::HandshakeComplete;
use actix::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use zznet_api::transport::TransportConnection;
use zznet_api::types::AuthContext;
use zznet_api::types::Role;
use zznet_session::peer_session::PeerSession;
use zznet_session::session_manager::SessionManager;
use zznet_session::types::{PeerId, RoomId};

type Authorizer = Box<dyn Fn(&AuthContext) -> Option<Role> + Send + Sync>;

/// ConnectionManager coordinates HelloActors and SessionManager
///
/// SECURITY: ConnectionManager REQUIRES an authorizer function.
/// There is no code path that allows connections without authorization.
/// Every peer must be explicitly authorized before gaining access.
///
/// Generic over TRole: the application's role type
pub struct ConnectionManager {
    /// SessionManager actor address for message-passing communication
    ///
    /// DESIGN: Uses Actix Addr<> for pure actor-based communication.
    /// - ConnectionManager sends messages to SessionManager actor
    /// - All operations use message passing (AddPeer, GetPeerIds, etc.)
    /// - Components receive their own Addr<SessionManager> for direct access
    /// - This enables concurrent access without blocking
    /// - Message passing provides natural backpressure and error handling
    session_manager: Addr<SessionManager>,

    /// Maps PeerId to HelloActor address
    /// Used to send InboundRoomMessage to the correct HelloActor
    hello_actors: HashMap<PeerId, Addr<HelloActor>>,
    /// REQUIRED: Authorizer function to resolve peer identity to role.
    /// This is NOT optional - every connection must be authorized.
    /// Takes PeerIdentity (from TLS certificate) and returns role if allowed.
    authorizer: Authorizer,
    /// Optional callback for wiring room handlers to newly-connected peers.
    /// Called from HandshakeComplete handler with the SessionManager address and new peer ID.
    /// IMPORTANT: This is now ASYNC and TRANSACTIONAL. If wiring fails, the connection
    /// is automatically terminated to prevent "zombie" connections.
    /// Returns a Result indicating success or failure of handler wiring.
    room_handler_wirer: Option<
        Arc<
            dyn Fn(
                    Addr<SessionManager>,
                    &PeerId,
                ) -> std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<(), String>> + Send>,
                > + Send
                + Sync,
        >,
    >,
}

impl ConnectionManager {
    /// Create a new ConnectionManager with a REQUIRED authorizer function.
    ///
    /// **DEPRECATED**: Use `new_with_session_manager()` instead. This constructor
    /// creates a SessionManager internally which cannot be shared with components.
    /// Create a new ConnectionManager with a provided SessionManager actor address.
    ///
    /// This is the CORRECT way to create ConnectionManager for production use.
    /// It ensures that the ConnectionManager uses the SAME SessionManager instance
    /// that components use, allowing proper message flow between network and components.
    ///
    /// SECURITY: An authorizer is MANDATORY.
    ///
    /// # Arguments
    /// - `session_manager`: SessionManager actor address (Addr<SessionManager<TRole>>)
    /// - `authorizer`: Function that maps PeerIdentity → Option<Role>
    ///
    /// # Design Principle
    /// "Per-Process Singleton: One SessionManager manages all connections for a process"
    /// - Create ONE SessionManager in the service initialization
    /// - Start it as an actor with `.start()` to get Addr<>
    /// - Pass the Addr to ALL components via clone()
    /// - Pass the Addr to ConnectionManager via this constructor
    /// - This ensures messages reach SessionManager from all sources
    pub fn new(session_manager: Addr<SessionManager>, authorizer: Authorizer) -> Self {
        Self {
            session_manager,
            hello_actors: HashMap::new(),
            authorizer,
            room_handler_wirer: None,
        }
    }

    /// Create a new ConnectionManager with a provided SessionManager actor address.
    ///
    /// Alias for `new()` - both names work the same way now.
    /// Use whichever name is clearer in your context.
    pub fn new_with_session_manager(
        session_manager: Addr<SessionManager>,
        authorizer: Authorizer,
    ) -> Self {
        Self::new(session_manager, authorizer)
    }

    /// Set the room handler wirer callback
    ///
    /// This callback is invoked when a new peer connects (after HandshakeComplete).
    /// It allows the application to wire room handlers to the new peer session.
    ///
    /// IMPORTANT: The wirer is now ASYNC and TRANSACTIONAL.
    /// - If wiring succeeds (returns Ok(())), the connection is considered complete
    /// - If wiring fails (returns Err(...)), the connection is terminated immediately
    /// - This prevents "zombie" connections where the transport is up but application logic isn't
    ///
    /// The wirer receives an Addr<SessionManager> for message-based communication.
    pub fn with_room_handler_wirer(
        mut self,
        wirer: Arc<
            dyn Fn(
                    Addr<SessionManager>,
                    &PeerId,
                ) -> std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<(), String>> + Send>,
                > + Send
                + Sync,
        >,
    ) -> Self {
        self.room_handler_wirer = Some(wirer);
        self
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
    peer_id: zznet_session::types::PeerId,
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
    peer_id: zznet_session::types::PeerId,
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

/// Handler for GetPeers - Forward to SessionManager via message passing
impl Handler<GetPeers> for ConnectionManager {
    type Result = ResponseFuture<Vec<PeerId>>;

    fn handle(&mut self, _msg: GetPeers, _ctx: &mut Context<Self>) -> Self::Result {
        let sm_addr = self.session_manager.clone();
        Box::pin(async move {
            sm_addr
                .send(zznet_session::messages::GetPeerIds)
                .await
                .unwrap_or_default()
        })
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

/// Handler for GetPeerSender - Forward to SessionManager via message passing
impl Handler<GetPeerSender> for ConnectionManager {
    type Result = ResponseFuture<Option<mpsc::Sender<(RoomId, Vec<u8>)>>>;

    fn handle(&mut self, msg: GetPeerSender, _ctx: &mut Context<Self>) -> Self::Result {
        let sm_addr = self.session_manager.clone();
        Box::pin(async move {
            sm_addr
                .send(zznet_session::messages::GetPeerSender {
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

/// Handler for SubscribePeerInbound - Forward to SessionManager via message passing
impl Handler<SubscribePeerInbound> for ConnectionManager {
    type Result = ResponseFuture<Option<tokio::sync::broadcast::Receiver<(RoomId, Vec<u8>)>>>;

    fn handle(&mut self, msg: SubscribePeerInbound, _ctx: &mut Context<Self>) -> Self::Result {
        let sm_addr = self.session_manager.clone();
        Box::pin(async move {
            sm_addr
                .send(zznet_session::messages::SubscribePeerInbound {
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
        // HELLO role is the PRIMARY source, TLS identity (if present) validates it.
        let auth_ctx = AuthContext {
            hello_role_str: msg.peer_role_str.clone(),
            peer_identity: if msg.peer_identity.common_name != "unused" {
                Some(msg.peer_identity.clone())
            } else {
                None
            },
        };

        match (self.authorizer)(&auth_ctx) {
            Some(role) => {
                tracing::info!(
                    "Peer {} authorized as {:?} (HELLO={}, identity={})",
                    peer_id,
                    role,
                    msg.peer_role_str,
                    auth_ctx
                        .peer_identity
                        .as_ref()
                        .map(|id| id.full_identity())
                        .unwrap_or_else(|| "none (plain TCP)".to_string())
                );

                // Prepare data to perform session mutations asynchronously without blocking the actor thread.
                let sm_addr = self.session_manager.clone();
                let hello_actor = msg.hello_actor.clone();
                let actor_addr = _ctx.address();
                let room_handler_wirer = self.room_handler_wirer.clone();

                // Create channels for SessionBridge
                let (outbound_tx, outbound_rx) = tokio::sync::mpsc::channel(100);
                let (conn_to_session_tx, conn_to_session_rx) = tokio::sync::mpsc::channel(100);
                let (hello_to_conn_tx, hello_to_conn_rx) = tokio::sync::mpsc::channel(100);

                // Spawn an async task to create connected peer session and add to SessionManager
                tokio::spawn(async move {
                    // Create peer session ALREADY CONNECTED (eliminates need for connect_peer call)
                    let peer_session_result = PeerSession::new_connected(
                        zznet_session::types::PeerId::from(peer_id.as_str()),
                        Some(role),
                        Some(msg.peer_identity.clone()),
                        outbound_tx.clone(),
                        conn_to_session_rx,
                    )
                    .await;

                    let peer_session = match peer_session_result {
                        Ok(ps) => ps,
                        Err(e) => {
                            tracing::error!(
                                "Failed to create connected peer session for {}: {:?}",
                                peer_id,
                                e
                            );
                            return;
                        }
                    };

                    // Add the fully-connected peer to SessionManager via message passing
                    let add_result = sm_addr
                        .send(zznet_session::messages::AddPeer {
                            peer_id: zznet_session::types::PeerId::from(peer_id.as_str()),
                            peer_session,
                        })
                        .await;

                    // Check the result
                    match add_result {
                        Ok(result) => {
                            if let Err(session_error) = result {
                                tracing::error!(
                                    "SessionManager rejected peer {}: {:?}",
                                    peer_id,
                                    session_error
                                );
                                return;
                            }
                        }
                        Err(mailbox_error) => {
                            tracing::error!(
                                "Failed to send AddPeer to SessionManager: {:?}",
                                mailbox_error
                            );
                            return;
                        }
                    }

                    // Wire room handlers for the newly-connected peer (transactional)
                    if let Some(wirer) = &room_handler_wirer {
                        match wirer(
                            sm_addr.clone(),
                            &zznet_session::types::PeerId::from(peer_id.as_str()),
                        )
                        .await
                        {
                            Ok(()) => {
                                // Wiring succeeded - connection is now fully functional
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Failed to wire room handlers for peer {}: {}. Terminating connection.",
                                    peer_id,
                                    e
                                );
                                // Wiring failed - disconnect this peer to prevent a "zombie" connection
                                if let Err(send_err) = sm_addr
                                    .send(zznet_session::messages::DisconnectPeer {
                                        peer_id: zznet_session::types::PeerId::from(
                                            peer_id.as_str(),
                                        ),
                                    })
                                    .await
                                {
                                    tracing::error!(
                                        "Failed to send disconnect message for peer {} after wiring failure: {:?}",
                                        peer_id,
                                        send_err
                                    );
                                }
                                // Send disconnect to HelloActor as well
                                hello_actor.do_send(crate::actor::Disconnect);
                                return;
                            }
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
                        peer_id: zznet_session::types::PeerId::from(peer_id.as_str()),
                        hello_actor: send_hello_actor,
                        outbound_rx: Some(outbound_rx),
                        conn_to_session_tx: conn_to_session_tx.clone(),
                        hello_to_conn_rx: Some(hello_to_conn_rx),
                    };

                    actor_addr.do_send(inner_msg);
                });
            }
            None => {
                // SECURITY: Authorization FAILED. Loud logging and immediate disconnect.
                tracing::error!(
                    "!!! SECURITY REJECTION !!!: Peer {} REJECTED by authorizer",
                    peer_id
                );
                tracing::error!(
                    "    HELLO claimed role: {} | TLS identity: {}",
                    msg.peer_role_str,
                    auth_ctx
                        .peer_identity
                        .as_ref()
                        .map(|id| id.full_identity())
                        .unwrap_or_else(|| "none (plain TCP)".to_string())
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

        // Create a mock authorizer that accepts all peers as Role::"admin"
        let authorizer = Box::new(|_peer_identity: &AuthContext| Some(Role::new("admin")))
            as Box<dyn Fn(&AuthContext) -> Option<Role> + Send + Sync>;

        // Create SessionManager and start it as an actor
        let session_manager = SessionManager::new(rooms).start();

        let _manager = ConnectionManager::new(session_manager, authorizer);
        // Just test it compiles and constructs
    }
}
