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
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use zznet_api::transport::TransportConnection;
use zznet_api::types::AuthContext;
use zznet_auth::ApplicationRole;
use zznet_session::peer_session::PeerSession;
use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::session_manager::SessionManager;
use zznet_session::types::{PeerId, RoomId};

type Authorizer<TRole> = Box<dyn Fn(&AuthContext) -> Option<TRole> + Send + Sync>;

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
    /// Manages all peer sessions (shared with components via Arc<Mutex<...>>)
    ///
    /// DESIGN: Wrapped in Arc<Mutex<...>> to enable sharing with components.
    /// - ConnectionManager needs &mut access for add_peer(), connect_peer()
    /// - Components need &self access for broadcast_to_room()
    /// - Arc<Mutex<...>> provides interior mutability for both use cases
    /// - All share the SAME SessionManager instance
    /// - This allows messages to flow: Network → SessionManager ← Components
    session_manager: Arc<Mutex<SessionManager<TMsg, TRole>>>,

    /// Maps PeerId to HelloActor address
    /// Used to send InboundRoomMessage to the correct HelloActor
    hello_actors: HashMap<PeerId, Addr<HelloActor>>,
    /// REQUIRED: Authorizer function to resolve peer identity to role.
    /// This is NOT optional - every connection must be authorized.
    /// Takes PeerIdentity (from TLS certificate) and returns role if allowed.
    authorizer: Authorizer<TRole>,
    /// Optional callback for wiring room handlers to newly-connected peers.
    /// Called from HandshakeComplete handler with the SessionManager and new peer ID.
    /// IMPORTANT: This is now ASYNC and TRANSACTIONAL. If wiring fails, the connection
    /// is automatically terminated to prevent "zombie" connections.
    /// Returns a Result indicating success or failure of handler wiring.
    room_handler_wirer: Option<
        Arc<
            dyn Fn(
                    Arc<Mutex<SessionManager<TMsg, TRole>>>,
                    &PeerId,
                ) -> std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<(), String>> + Send>,
                > + Send
                + Sync,
        >,
    >,
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
            session_manager: Arc::new(Mutex::new(SessionManager::new_with_limits(
                offered_rooms,
                None,
                None,
            ))),
            hello_actors: HashMap::new(),
            authorizer,
            room_handler_wirer: None,
        }
    }

    /// Create a new ConnectionManager with a provided shared SessionManager.
    ///
    /// This is the CORRECT way to create ConnectionManager for production use.
    /// It ensures that the ConnectionManager shares the SAME SessionManager instance
    /// that components use, allowing proper message flow between network and components.
    ///
    /// SECURITY: An authorizer is MANDATORY.
    ///
    /// # Arguments
    /// - `session_manager`: Shared SessionManager instance (Arc<Mutex<...>>-wrapped)
    /// - `authorizer`: Function that maps PeerIdentity → Option<Role>
    ///
    /// # Design Principle
    /// "Per-Process Singleton: One SessionManager manages all connections for a process"
    /// - Create ONE SessionManager in the service initialization
    /// - Wrap it in Arc<Mutex<...>>
    /// - Pass it to ALL components via Arc::clone()
    /// - Pass it to ConnectionManager via this constructor
    /// - This ensures messages broadcast to SessionManager reach all components
    pub fn new_with_session_manager(
        session_manager: Arc<Mutex<SessionManager<TMsg, TRole>>>,
        authorizer: Authorizer<TRole>,
    ) -> Self {
        Self {
            session_manager,
            hello_actors: HashMap::new(),
            authorizer,
            room_handler_wirer: None,
        }
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
    pub fn with_room_handler_wirer(
        mut self,
        wirer: Arc<
            dyn Fn(
                    Arc<Mutex<SessionManager<TMsg, TRole>>>,
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
    /// Add a pre-configured PeerSession to the SessionManager
    ///
    /// This allows the application to create Room<T> instances with
    /// different T types before adding to the session.
    pub fn add_peer(&mut self, peer_id: PeerId, peer_session: PeerSession<TMsg, TRole>) {
        if let Err(e) = self
            .session_manager
            .blocking_lock()
            .add_peer(peer_id.clone(), peer_session)
        {
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
    /// Get the sender for sending messages to a peer
    ///
    /// Returns `None` if peer doesn't exist or isn't connected.
    pub fn get_peer_sender(&self, peer_id: &PeerId) -> Option<mpsc::Sender<(RoomId, TMsg)>> {
        self.session_manager
            .try_lock()
            .ok()
            .and_then(|sm| sm.get_peer_sender(peer_id))
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
        self.session_manager
            .try_lock()
            .ok()
            .and_then(|mut sm| sm.subscribe_peer_inbound(peer_id))
    }

    /// Get list of connected peer IDs
    pub fn peer_ids(&self) -> Vec<PeerId> {
        self.session_manager
            .try_lock()
            .map(|sm| sm.peer_ids())
            .unwrap_or_default()
    }
}

impl<TMsg, TRole> Actor for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
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

impl<TMsg, TRole> Handler<HandshakePostProcessed> for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    type Result = ();

    fn handle(&mut self, msg: HandshakePostProcessed, _ctx: &mut Context<Self>) {
        self.hello_actors.insert(msg.peer_id, msg.hello_actor);
    }
}

/// Internal message carrying the channels to start a SessionBridge inside the actor context.
#[derive(Message)]
#[rtype(result = "()")]
struct HandshakePostProcessedInner<TMsg> {
    peer_id: zznet_session::types::PeerId,
    hello_actor: Addr<HelloActor>,
    outbound_rx: Option<tokio::sync::mpsc::Receiver<(RoomId, TMsg)>>,
    conn_to_session_tx: tokio::sync::mpsc::Sender<(RoomId, TMsg)>,
    hello_to_conn_rx: Option<tokio::sync::mpsc::Receiver<(String, Vec<u8>)>>,
}

impl<TMsg, TRole> Handler<HandshakePostProcessedInner<TMsg>> for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait + 'static,
    TRole: ApplicationRole,
{
    type Result = ();

    fn handle(&mut self, msg: HandshakePostProcessedInner<TMsg>, _ctx: &mut Context<Self>) {
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

impl<TMsg, TRole> Handler<HandleTransport> for ConnectionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
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
                let sm = Arc::clone(&self.session_manager);
                let hello_actor = msg.hello_actor.clone();
                let actor_addr = _ctx.address();
                let room_handler_wirer = self.room_handler_wirer.clone();

                // Create PeerSession for this peer and set auth context
                let mut peer_session =
                    PeerSession::new(zznet_session::types::PeerId::from(peer_id.as_str()));
                peer_session.set_role(Some(role));
                peer_session.set_identity(msg.peer_identity.clone());

                // Create channels for SessionBridge
                let (outbound_tx, outbound_rx) = tokio::sync::mpsc::channel(100);
                let (conn_to_session_tx, conn_to_session_rx) = tokio::sync::mpsc::channel(100);
                let (hello_to_conn_tx, hello_to_conn_rx) = tokio::sync::mpsc::channel(100);

                // Spawn an async task to mutate the SessionManager. After this completes
                // we'll send the channels back to the actor so it can start the SessionBridge
                // from within the actor context (avoids spawn_local runtime issues).
                tokio::spawn(async move {
                    // Lock the session manager asynchronously
                    let mut guard = sm.lock().await;

                    if let Err(e) = guard.add_peer(
                        zznet_session::types::PeerId::from(peer_id.as_str()),
                        peer_session,
                    ) {
                        tracing::error!(
                            "Failed to add peer {} to SessionManager: {:?}",
                            peer_id,
                            e
                        );
                        return;
                    }

                    if let Err(e) = guard
                        .connect_peer(
                            zznet_session::types::PeerId::from(peer_id.as_str()),
                            outbound_tx.clone(),
                            conn_to_session_rx,
                        )
                        .await
                    {
                        tracing::error!(
                            "Failed to connect peer {} in SessionManager: {:?}",
                            peer_id,
                            e
                        );
                        return;
                    }

                    // Release the lock BEFORE calling the wirer to avoid deadlocks
                    // (the wirer might need to lock the SessionManager again)
                    drop(guard);

                    // Wire room handlers for the newly-connected peer (transactional)
                    if let Some(wirer) = &room_handler_wirer {
                        match wirer(
                            Arc::clone(&sm),
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
                                let mut guard = sm.lock().await;
                                if let Err(e) = guard.disconnect_peer(
                                    &zznet_session::types::PeerId::from(peer_id.as_str()),
                                ) {
                                    tracing::error!(
                                        "Failed to disconnect peer {} after wiring failure: {:?}",
                                        peer_id,
                                        e
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
                    let inner_msg = HandshakePostProcessedInner::<TMsg> {
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

        // Create a mock authorizer that accepts all peers as MockRole::Admin
        let authorizer: Authorizer<zznet_auth::mock::MockRole> =
            Box::new(|_peer_identity| Some(zznet_auth::mock::MockRole::Admin));

        let _manager =
            ConnectionManager::<TestMessages, zznet_auth::mock::MockRole>::new(rooms, authorizer);
        // Just test it compiles and constructs
    }
}
