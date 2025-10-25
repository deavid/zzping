use crate::peer_session::{PeerSession, RoomHandle};
use crate::types::{ConnectionState, PeerId, RoomId, SessionError};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing;

// NEW: Auth imports
use crate::session_manager_like::SessionManagerLike;
use async_trait::async_trait;
use zznet_api::types::PeerIdentity;
use zznet_auth::ApplicationRole;

// NEW: Room registration imports
use zznet_room::RoomRegistry;

#[async_trait]
impl<TRole> SessionManagerLike<TRole> for SessionManager<TRole>
where
    TRole: ApplicationRole,
{
    async fn send_to_room(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError> {
        <Self>::send_to_room(self, peer_id, room_id, bytes).await
    }

    fn get_peer_role(&self, peer_id: &PeerId) -> Option<TRole> {
        // Return a cloned role if present. This requires TRole: Clone which is
        // enforced on the trait declaration of SessionManagerLike.
        self.get_peer_role_cloned(peer_id)
    }
}

/// Manages all peer sessions for this process
///
/// **Architecture Change**: No longer generic over TMsg. All messages are Vec<u8>.
/// Room<T> handles serialization internally, so SessionManager works purely with bytes.
///
/// 100% byte-based messages. Messages are serialized at the Room level, not here.
pub struct SessionManager<TRole>
where
    TRole: ApplicationRole,
{
    /// All peer sessions
    peers: HashMap<PeerId, PeerSession<TRole>>,

    /// Rooms this SessionManager offers
    /// Used during PublishRooms negotiation to compute intersection with peers
    offered_rooms: Vec<RoomId>,

    /// Optional maximum number of peers this manager will accept
    max_peers: Option<usize>,

    /// Optional maximum number of rooms per peer
    max_rooms_per_peer: Option<usize>,

    /// Room handlers registered for auto-registration
    /// Maps room_id -> (inbound_tx, outbound_rx) channels
    /// These are stored and activated when peers connect
    room_handlers: HashMap<RoomId, (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>)>,
}

impl<TRole> SessionManager<TRole>
where
    TRole: ApplicationRole,
{
    /// Create a new SessionManager
    ///
    /// `offered_rooms`: List of room IDs this manager offers
    pub fn new(offered_rooms: Vec<RoomId>) -> Self {
        tracing::info!(
            "SessionManager configured with {} offered rooms: {:?}",
            offered_rooms.len(),
            offered_rooms
        );
        Self {
            peers: HashMap::new(),
            offered_rooms,
            max_peers: None,
            max_rooms_per_peer: None,
            room_handlers: HashMap::new(),
        }
    }

    /// Create a new SessionManager with explicit connection limits
    pub fn new_with_limits(
        offered_rooms: Vec<RoomId>,
        max_peers: Option<usize>,
        max_rooms_per_peer: Option<usize>,
    ) -> Self {
        tracing::info!(
            "SessionManager configured with {} offered rooms: {:?} (max_peers: {:?}, max_rooms_per_peer: {:?})",
            offered_rooms.len(),
            offered_rooms,
            max_peers,
            max_rooms_per_peer
        );
        Self {
            peers: HashMap::new(),
            offered_rooms,
            max_peers,
            max_rooms_per_peer,
            room_handlers: HashMap::new(),
        }
    }

    /// Add a new peer session (initially disconnected)
    ///
    /// The caller must construct the PeerSession with its rooms already added.
    /// This allows the application to create Room<T> instances with different T types
    /// and type-erase them to Box<dyn RoomHandle> before adding to the session.
    ///
    /// `peer_id`: Unique identifier for this peer
    /// `peer_session`: Pre-configured peer session with rooms
    pub fn add_peer(
        &mut self,
        peer_id: PeerId,
        peer_session: PeerSession<TRole>,
    ) -> Result<(), SessionError> {
        if self.peers.contains_key(&peer_id) {
            return Err(SessionError::PeerAlreadyExists(peer_id));
        }

        // Enforce global peer limit if configured
        match self.max_peers {
            Some(max) if self.peers.len() >= max => {
                return Err(SessionError::PeerLimitExceeded { max });
            }
            _ => {}
        }

        // Enforce per-peer room limit if configured. We check the pre-configured
        // rooms on the PeerSession so callers that add many rooms before registering
        // the peer are also constrained.
        if let Some(max_rooms) = self.max_rooms_per_peer {
            let room_count = peer_session.room_ids().len();
            if room_count > max_rooms {
                return Err(SessionError::RoomLimitExceeded {
                    peer_id: peer_id.clone(),
                    max: max_rooms,
                });
            }
        }

        self.peers.insert(peer_id, peer_session);
        Ok(())
    }

    // NOTE: disconnect_peer removed — prefer explicit peer.disconnect() or
    // using actor messages. The previous implementation closed channels and
    // aborted tasks but kept the peer in the map; callers should now either
    // call `remove_peer()` (to remove) or obtain a mutable reference to the
    // peer and call `peer.disconnect()` directly when appropriate.

    /// Remove a peer entirely
    /// Disconnects and removes from map
    pub fn remove_peer(&mut self, peer_id: &PeerId) -> Result<(), SessionError> {
        let peer = self
            .peers
            .remove(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))?;

        drop(peer); // Ensures cleanup

        tracing::info!("Removed peer: {}", peer_id);
        Ok(())
    }

    /// Add a room handler to an existing peer
    ///
    /// This allows components to register handlers for rooms after the peer has been connected.
    /// Useful for wiring component-specific room handlers to receive messages from peers.
    pub async fn add_room_to_peer(
        &mut self,
        peer_id: &PeerId,
        room_id: RoomId,
        room_handle: Box<dyn RoomHandle>,
    ) -> Result<(), SessionError> {
        let peer = self
            .peers
            .get_mut(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))?;

        peer.add_room(room_id.clone(), room_handle).await?;

        tracing::debug!("Added room {:?} to peer {}", room_id, peer_id);
        Ok(())
    }

    /// Get the connection state of a peer
    pub fn peer_state(&self, peer_id: &PeerId) -> Option<ConnectionState> {
        self.peers.get(peer_id).map(|p| p.state())
    }

    /// Check if a peer is connected
    pub fn is_peer_connected(&self, peer_id: &PeerId) -> bool {
        self.peers
            .get(peer_id)
            .map(|p| p.is_connected())
            .unwrap_or(false)
    }

    /// Send a typed message to a specific peer's room
    ///
    /// The message is already serialized (Vec<u8>).
    /// Serialization happens at the Room layer.
    pub async fn send_to_room(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError> {
        let peer = self
            .peers
            .get(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))?;

        peer.send_to_room(room_id, bytes).await
    }

    /// Get list of all peer IDs
    pub fn peer_ids(&self) -> Vec<PeerId> {
        self.peers.keys().cloned().collect()
    }

    /// Get the authenticated role for a peer
    ///
    /// Returns None if:
    /// - Peer doesn't exist
    /// - Peer has no role set (ACL not configured)
    ///
    /// # Arguments
    /// * `peer_id` - The peer to query
    pub fn get_peer_role(&self, peer_id: &PeerId) -> Option<&TRole> {
        self.peers.get(peer_id)?.role()
    }

    /// Convenience clone-returning wrapper for SessionManagerLike consumers.
    ///
    /// This returns an owned TRole if present. It is primarily intended for
    /// places where the underlying SessionManagerLike trait is used and a
    /// simple ownership-semantics helper is handy.
    pub fn get_peer_role_cloned(&self, peer_id: &PeerId) -> Option<TRole>
    where
        TRole: Clone,
    {
        self.get_peer_role(peer_id).cloned()
    }

    /// Get the full identity information for a peer
    ///
    /// Returns None if:
    /// - Peer doesn't exist
    /// - Peer has no identity set
    ///
    /// # Arguments
    /// * `peer_id` - The peer to query
    pub fn get_peer_identity(&self, peer_id: &PeerId) -> Option<&PeerIdentity> {
        self.peers.get(peer_id)?.identity()
    }

    /// Get all peers with a specific role
    ///
    /// Useful for filtering operations (e.g., "send to all Collectors")
    ///
    /// # Arguments
    /// * `role` - The role to filter by
    ///
    /// # Returns
    /// Vector of peer IDs that have the specified role
    pub fn peers_with_role(&self, role: &TRole) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter_map(|(id, session)| {
                if session.role() == Some(role) {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get number of connected peers
    pub fn connected_peer_count(&self) -> usize {
        self.peers.values().filter(|p| p.is_connected()).count()
    }

    /// Get the list of offered rooms
    pub fn offered_rooms(&self) -> &[RoomId] {
        &self.offered_rooms
    }

    /// Get a cloneable sender for a specific peer
    ///
    /// This allows application code to send messages to a peer without going through
    /// the actor system. The returned `mpsc::Sender` can be cloned and used from any
    /// async context.
    ///
    /// Returns `None` if the peer doesn't exist or isn't connected.
    ///
    pub fn get_peer_sender(&self, peer_id: &PeerId) -> Option<mpsc::Sender<(RoomId, Vec<u8>)>> {
        let peer = self.peers.get(peer_id)?;
        peer.get_sender()
    }

    /// Subscribe to inbound messages from a specific peer
    ///
    /// Returns a broadcast receiver that will receive all inbound messages from the peer.
    /// This is useful for clients that want to handle messages directly without using
    /// the Room abstraction (e.g., request-response patterns).
    ///
    /// Multiple subscribers can call this method to get independent receivers.
    ///
    /// Returns `None` if the peer doesn't exist or is not connected.
    pub fn subscribe_peer_inbound(
        &mut self,
        peer_id: &PeerId,
    ) -> Option<tokio::sync::broadcast::Receiver<(RoomId, Vec<u8>)>> {
        let peer = self.peers.get_mut(peer_id)?;
        peer.subscribe_inbound()
    }

    /// Set the rooms this SessionManager offers to all peers
    ///
    /// This is typically called at startup to declare which rooms
    /// this process supports. When PublishRooms messages are exchanged,
    /// these offered rooms are used to compute the intersection with
    /// each peer's offered rooms.
    pub fn set_offered_rooms(&mut self, rooms: Vec<RoomId>) {
        self.offered_rooms = rooms;
    }

    /// Handle PublishRooms message from a peer
    ///
    /// Propagates the peer's offered rooms to their PeerSession for negotiation.
    /// Computes the intersection of rooms and returns the list of joined rooms.
    pub fn handle_publish_rooms(
        &mut self,
        peer_id: &PeerId,
        peer_rooms: Vec<RoomId>,
    ) -> Result<Vec<RoomId>, SessionError> {
        let peer = self
            .peers
            .get_mut(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))?;

        // Propagate to PeerSession for negotiation
        peer.handle_peer_offered_rooms(peer_rooms)?;

        // Return the intersection
        Ok(peer.joined_rooms().to_vec())
    }

    /// Get the rooms joined with a specific peer (after negotiation)
    ///
    /// Returns the intersection of rooms that were negotiated via
    /// `handle_publish_rooms()`. This is the list of rooms that can
    /// be used for communication with this peer.
    pub fn peer_joined_rooms(&self, peer_id: &PeerId) -> Result<&[RoomId], SessionError> {
        let peer = self
            .peers
            .get(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))?;

        Ok(peer.joined_rooms())
    }

    /// Check if a specific room is joined with a peer
    ///
    /// This is a convenience method that checks if a room_id is in the
    /// joined rooms list for the specified peer.
    pub fn is_room_joined_with_peer(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
    ) -> Result<bool, SessionError> {
        let peer = self
            .peers
            .get(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))?;

        Ok(peer.is_room_joined(room_id))
    }
}

/// RoomRegistry implementation for SessionManager
/// Allows Room<T> instances to auto-register their channels with SessionManager
impl<TRole> RoomRegistry for SessionManager<TRole>
where
    TRole: ApplicationRole,
{
    fn register_room_handler(
        &mut self,
        room_id: String,
        inbound_tx: mpsc::Sender<Vec<u8>>,
        outbound_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let room_id_typed = RoomId::new(room_id.clone());

        // Check if room is already registered
        if self.room_handlers.contains_key(&room_id_typed) {
            return Err(format!("Room {} already registered", room_id).into());
        }

        // Store the channels for later activation when peers connect
        self.room_handlers
            .insert(room_id_typed, (inbound_tx, outbound_rx));

        tracing::debug!("Registered room handler for room_id: {}", room_id);

        Ok(())
    }
}

// ============================================================================
// Actor Implementation
// ============================================================================

use crate::messages::{
    AddPeer, BroadcastToRole, DisconnectPeer, GetConnectedPeerCount, GetOfferedRooms,
    GetPeerIdentity, GetPeerIds, GetPeerJoinedRooms, GetPeerRole, GetPeerSender, GetPeerState,
    GetPeersWithRole, HandlePublishRooms, IsPeerConnected, IsRoomJoinedWithPeer, RemovePeer,
    SendToRoom, SetOfferedRooms, SubscribePeerInbound,
};
use actix::prelude::*;

impl<TRole> Actor for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::info!("SessionManager actor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::info!("SessionManager actor stopped");
    }
}

// ============================================================================
// Message Handlers - Peer Management
// ============================================================================
//
// NOTE: ConnectPeer handler is NOT implemented due to fundamental Rust
// lifetime constraints:
// - peer.connect() is async and borrows &mut self
// - Actor handlers must return 'static futures
// - PeerSession fields are private, so we cannot inline the connect logic
//
// WORKAROUND: ConnectionManager will continue using Arc<Mutex<SessionManager>>
// temporarily alongside Addr<SessionManager> for operations that work via
// messages. This hybrid approach allows Phase 2 migration to proceed.

impl<TRole> Handler<AddPeer<TRole>> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Result<(), SessionError>;

    fn handle(&mut self, msg: AddPeer<TRole>, _ctx: &mut Context<Self>) -> Self::Result {
        self.add_peer(msg.peer_id, msg.peer_session)
    }
}

// ConnectPeer handler - NOT implemented (see note above)

impl<TRole> Handler<DisconnectPeer> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Result<(), SessionError>;

    fn handle(&mut self, msg: DisconnectPeer, _ctx: &mut Context<Self>) -> Self::Result {
        let peer = self
            .peers
            .get_mut(&msg.peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(msg.peer_id.clone()))?;

        peer.disconnect();

        tracing::info!("Disconnected from peer: {}", msg.peer_id);
        Ok(())
    }
}

impl<TRole> Handler<RemovePeer> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Result<(), SessionError>;

    fn handle(&mut self, msg: RemovePeer, _ctx: &mut Context<Self>) -> Self::Result {
        self.remove_peer(&msg.peer_id)
    }
}

// AddRoomToPeer handler - SKIPPED for Phase 1 (see note above)

// ============================================================================
// Message Handlers - Room Management
// ============================================================================

impl<TRole> Handler<SetOfferedRooms> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = ();

    fn handle(&mut self, msg: SetOfferedRooms, _ctx: &mut Context<Self>) -> Self::Result {
        self.set_offered_rooms(msg.rooms)
    }
}

impl<TRole> Handler<HandlePublishRooms> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Result<Vec<RoomId>, SessionError>;

    fn handle(&mut self, msg: HandlePublishRooms, _ctx: &mut Context<Self>) -> Self::Result {
        self.handle_publish_rooms(&msg.peer_id, msg.requested_rooms)
    }
}

// ============================================================================
// Message Handlers - Query Operations
// ============================================================================

impl<TRole> Handler<GetPeerState> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Option<ConnectionState>;

    fn handle(&mut self, msg: GetPeerState, _ctx: &mut Context<Self>) -> Self::Result {
        self.peer_state(&msg.peer_id)
    }
}

impl<TRole> Handler<IsPeerConnected> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = bool;

    fn handle(&mut self, msg: IsPeerConnected, _ctx: &mut Context<Self>) -> Self::Result {
        self.is_peer_connected(&msg.peer_id)
    }
}

impl<TRole> Handler<GetPeerIds> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Vec<PeerId>;

    fn handle(&mut self, _msg: GetPeerIds, _ctx: &mut Context<Self>) -> Self::Result {
        self.peer_ids()
    }
}

impl<TRole> Handler<GetPeerRole<TRole>> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Option<TRole>;

    fn handle(&mut self, msg: GetPeerRole<TRole>, _ctx: &mut Context<Self>) -> Self::Result {
        self.get_peer_role_cloned(&msg.peer_id)
    }
}

impl<TRole> Handler<GetPeerIdentity> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Option<PeerIdentity>;

    fn handle(&mut self, msg: GetPeerIdentity, _ctx: &mut Context<Self>) -> Self::Result {
        self.get_peer_identity(&msg.peer_id).cloned()
    }
}

impl<TRole> Handler<GetPeersWithRole<TRole>> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Vec<PeerId>;

    fn handle(&mut self, msg: GetPeersWithRole<TRole>, _ctx: &mut Context<Self>) -> Self::Result {
        self.peers_with_role(&msg.role)
    }
}

impl<TRole> Handler<GetConnectedPeerCount> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = usize;

    fn handle(&mut self, _msg: GetConnectedPeerCount, _ctx: &mut Context<Self>) -> Self::Result {
        self.connected_peer_count()
    }
}

impl<TRole> Handler<GetOfferedRooms> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Vec<RoomId>;

    fn handle(&mut self, _msg: GetOfferedRooms, _ctx: &mut Context<Self>) -> Self::Result {
        self.offered_rooms().to_vec()
    }
}

impl<TRole> Handler<GetPeerSender> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Option<mpsc::Sender<(RoomId, Vec<u8>)>>;

    fn handle(&mut self, msg: GetPeerSender, _ctx: &mut Context<Self>) -> Self::Result {
        self.get_peer_sender(&msg.peer_id)
    }
}

impl<TRole> Handler<SubscribePeerInbound> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Option<tokio::sync::broadcast::Receiver<(RoomId, Vec<u8>)>>;

    fn handle(&mut self, msg: SubscribePeerInbound, _ctx: &mut Context<Self>) -> Self::Result {
        self.subscribe_peer_inbound(&msg.peer_id)
    }
}

impl<TRole> Handler<GetPeerJoinedRooms> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Result<Vec<RoomId>, SessionError>;

    fn handle(&mut self, msg: GetPeerJoinedRooms, _ctx: &mut Context<Self>) -> Self::Result {
        self.peer_joined_rooms(&msg.peer_id)
            .map(|rooms| rooms.to_vec())
    }
}

impl<TRole> Handler<IsRoomJoinedWithPeer> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Result<bool, SessionError>;

    fn handle(&mut self, msg: IsRoomJoinedWithPeer, _ctx: &mut Context<Self>) -> Self::Result {
        self.is_room_joined_with_peer(&msg.peer_id, &msg.room_id)
    }
}

// ============================================================================
// Message Handlers - Message Sending
// ============================================================================
//
// SendToRoom handler - Currently implemented with workaround for lifetime issues
// BroadcastToRole - Optimized batch operation

impl<TRole> Handler<SendToRoom> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = ResponseActFuture<Self, Result<(), SessionError>>;

    fn handle(&mut self, msg: SendToRoom, _ctx: &mut Context<Self>) -> Self::Result {
        let peer_id = msg.peer_id.clone();
        let room_id = msg.room_id;
        let bytes = msg.bytes;

        // Inline the send_to_room logic to avoid lifetime issues
        let result = match self.peers.get(&peer_id) {
            None => Err(SessionError::PeerNotFound(peer_id)),
            Some(peer) => {
                if !peer.is_connected() {
                    Err(SessionError::PeerNotConnected(peer_id))
                } else if !peer.is_room_joined(&room_id) {
                    Err(SessionError::RoomNotJoined(room_id))
                } else {
                    // Get the sender and send the message
                    match peer.get_sender() {
                        Some(tx) => {
                            // Send asynchronously
                            let fut = async move {
                                tx.send((room_id, bytes))
                                    .await
                                    .map_err(|_| SessionError::SendFailed)
                            };
                            return Box::pin(actix::fut::wrap_future(fut));
                        }
                        None => Err(SessionError::PeerNotConnected(peer_id)),
                    }
                }
            }
        };

        Box::pin(actix::fut::ready(result))
    }
}

// ============================================================================
// Message Handlers - Optimized Batch Operations
// ============================================================================

impl<TRole> Handler<BroadcastToRole<TRole>> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = ResponseFuture<Result<(), SessionError>>;

    fn handle(&mut self, msg: BroadcastToRole<TRole>, _ctx: &mut Context<Self>) -> Self::Result {
        let peers = self.peers_with_role(&msg.role);
        let room_id = msg.room_id;
        let bytes = msg.bytes;

        // Collect senders for all peers with the role
        let mut senders = Vec::new();
        for peer_id in &peers {
            if let Some(peer) = self.peers.get(peer_id)
                && peer.is_connected()
                && peer.is_room_joined(&room_id)
                && let Some(tx) = peer.get_sender()
            {
                senders.push((peer_id.clone(), tx));
            }
        }

        // Send to all collected senders
        Box::pin(async move {
            for (peer_id, tx) in senders {
                tx.send((room_id.clone(), bytes.clone()))
                    .await
                    .map_err(|_| SessionError::SendFailed)?;
                tracing::debug!("Broadcast sent to peer {:?} in room {:?}", peer_id, room_id);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use zznet_auth::mock::MockRole;

    // NOTE: tests below create PeerSession instances via
    // `PeerSession::new_connected(...).await.unwrap()` and call
    // `disconnect()` where a disconnected session is required before
    // adding to SessionManager. Constructing PeerSession directly would
    // require accessing private fields, so we avoid that here.

    #[actix::test]
    async fn test_session_manager_new() {
        let offered_rooms = vec![RoomId::from("intentconfig"), RoomId::from("memdb")];
        let manager = SessionManager::<MockRole>::new(offered_rooms.clone());

        assert_eq!(manager.offered_rooms(), offered_rooms.as_slice());
        assert_eq!(manager.peer_ids().len(), 0);
        assert_eq!(manager.connected_peer_count(), 0);
    }

    #[actix::test]
    async fn test_add_peer() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);
        let peer_id = PeerId::from("test_peer");

        // Create empty peer session (create connected then disconnect)
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let (_tx2, rx2) = tokio::sync::mpsc::channel(100);
        let mut peer_session = PeerSession::new_connected(peer_id.clone(), None, None, tx, rx2)
            .await
            .unwrap();
        peer_session.disconnect();

        manager.add_peer(peer_id.clone(), peer_session).unwrap();

        assert_eq!(manager.peer_ids(), vec![peer_id.clone()]);
        assert_eq!(
            manager.peer_state(&peer_id),
            Some(ConnectionState::Disconnected)
        );
    }

    #[actix::test]
    async fn test_add_peer_already_exists() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);
        let peer_id = PeerId::from("test_peer");

        // Add first time (create, then disconnect)
        let (tx1, _rx1) = tokio::sync::mpsc::channel(100);
        let (_tx2_1, rx2_1) = tokio::sync::mpsc::channel(100);
        let mut peer_session1 = PeerSession::new_connected(peer_id.clone(), None, None, tx1, rx2_1)
            .await
            .unwrap();
        peer_session1.disconnect();
        manager.add_peer(peer_id.clone(), peer_session1).unwrap();

        // Try to add again
        let (tx2, _rx2) = tokio::sync::mpsc::channel(100);
        let (_tx2_2, rx2_2) = tokio::sync::mpsc::channel(100);
        let mut peer_session2 = PeerSession::new_connected(peer_id.clone(), None, None, tx2, rx2_2)
            .await
            .unwrap();
        peer_session2.disconnect();
        let result = manager.add_peer(peer_id.clone(), peer_session2);
        assert!(matches!(result, Err(SessionError::PeerAlreadyExists(_))));
    }

    #[actix::test]
    async fn test_remove_peer() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);
        let peer_id = PeerId::from("test_peer");

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let (_tx2, rx2) = tokio::sync::mpsc::channel(100);
        let peer_session = PeerSession::new_connected(peer_id.clone(), None, None, tx, rx2)
            .await
            .unwrap();
        manager.add_peer(peer_id.clone(), peer_session).unwrap();
        assert_eq!(manager.peer_ids().len(), 1);

        manager.remove_peer(&peer_id).unwrap();
        assert_eq!(manager.peer_ids().len(), 0);
    }

    #[actix::test]
    async fn test_remove_peer_not_found() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);
        let peer_id = PeerId::from("nonexistent_peer");

        let result = manager.remove_peer(&peer_id);
        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_peer_ids() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);
        let peer_id1 = PeerId::from("peer1");
        let peer_id2 = PeerId::from("peer2");

        let (tx1, _rx1) = tokio::sync::mpsc::channel(100);
        let (_tx2_1, rx2_1) = tokio::sync::mpsc::channel(100);
        let peer_session1 = PeerSession::new_connected(peer_id1.clone(), None, None, tx1, rx2_1)
            .await
            .unwrap();
        let (tx2, _rx2) = tokio::sync::mpsc::channel(100);
        let (_tx2_2, rx2_2) = tokio::sync::mpsc::channel(100);
        let peer_session2 = PeerSession::new_connected(peer_id2.clone(), None, None, tx2, rx2_2)
            .await
            .unwrap();

        manager.add_peer(peer_id1.clone(), peer_session1).unwrap();
        manager.add_peer(peer_id2.clone(), peer_session2).unwrap();

        let mut peer_ids = manager.peer_ids();
        peer_ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));

        assert_eq!(peer_ids, vec![peer_id1, peer_id2]);
    }

    #[actix::test]
    async fn test_offered_rooms() {
        let offered_rooms = vec![RoomId::from("intentconfig"), RoomId::from("memdb")];
        let manager = SessionManager::<MockRole>::new(offered_rooms.clone());

        assert_eq!(manager.offered_rooms(), offered_rooms.as_slice());
    }

    // Test removed: connect_peer() method has been deprecated and removed.
    // Use PeerSession::new_connected() instead.

    #[actix::test]
    async fn test_disconnect_peer_not_found() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);
        let peer_id = PeerId::from("nonexistent_peer");

        // `disconnect_peer` was removed; removing a non-existent peer should
        // return PeerNotFound as well, so exercise `remove_peer` here.
        let result = manager.remove_peer(&peer_id);
        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_send_to_room_peer_not_found() {
        let manager = SessionManager::<MockRole>::new(vec![]);
        let peer_id = PeerId::from("nonexistent_peer");
        let room_id = RoomId::from("intentconfig");
        let msg = [0u8].to_vec();

        let result = manager.send_to_room(&peer_id, &room_id, msg).await;
        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    // Test removed: test_basic_peer_lifecycle tested connect/disconnect which no longer
    // exists with the new pattern. Peers are created already connected via new_connected().

    // Test removed: test_multiple_peers_simultaneously tested connect/disconnect transitions
    // which no longer exist. Peers are created already connected via new_connected().

    // --- New tests for limits ---

    #[actix::test]
    async fn test_peer_limit_exceeded() {
        // Create manager with max_peers = 2
        let mut manager = SessionManager::<MockRole>::new_with_limits(vec![], Some(2), None);

        // Add two peers - should succeed
        for id in ["peer1", "peer2"] {
            let (tx, _rx) = tokio::sync::mpsc::channel(100);
            let (_tx2, rx2) = tokio::sync::mpsc::channel(100);
            let peer_session = PeerSession::new_connected(PeerId::from(id), None, None, tx, rx2)
                .await
                .unwrap();
            manager.add_peer(PeerId::from(id), peer_session).unwrap();
        }

        // Third peer should fail
        let (tx_tmp, _rx_tmp) = tokio::sync::mpsc::channel(100);
        let (_tx2_tmp, rx2_tmp) = tokio::sync::mpsc::channel(100);
        let peer3 = PeerSession::new_connected(PeerId::from("peer3"), None, None, tx_tmp, rx2_tmp)
            .await
            .unwrap();
        let res = manager.add_peer(PeerId::from("peer3"), peer3);
        assert!(matches!(res, Err(SessionError::PeerLimitExceeded { .. })));
    }

    #[actix::test]
    async fn test_room_limit_exceeded_on_add_peer() {
        // Create manager with max_rooms_per_peer = 1
        let mut manager = SessionManager::<MockRole>::new_with_limits(vec![], None, Some(1));

        // Create peer session and add two simple mock rooms before registering
        let (tx_tmp, _rx_tmp) = tokio::sync::mpsc::channel(100);
        let (_tx2_tmp, rx2_tmp) = tokio::sync::mpsc::channel(100);
        let mut peer = PeerSession::new_connected(PeerId::from("p1"), None, None, tx_tmp, rx2_tmp)
            .await
            .unwrap();

        // Simple mock RoomHandle implementation for tests
        struct SimpleRoom {
            id: RoomId,
        }

        impl SimpleRoom {
            fn new(id: &str) -> Self {
                Self {
                    id: RoomId::from(id),
                }
            }
        }

        impl crate::peer_session::RoomHandle for SimpleRoom {
            fn room_id(&self) -> &RoomId {
                &self.id
            }

            fn send_message(&mut self, _msg: Vec<u8>) -> Result<(), SessionError> {
                Ok(())
            }

            fn spawn_forwarder(
                &mut self,
                _tx: mpsc::Sender<(RoomId, Vec<u8>)>,
            ) -> Result<(), SessionError> {
                Ok(())
            }
        }

        // Add two rooms to the peer
        peer.add_room(RoomId::from("r1"), Box::new(SimpleRoom::new("r1")))
            .await
            .unwrap();
        peer.add_room(RoomId::from("r2"), Box::new(SimpleRoom::new("r2")))
            .await
            .unwrap();

        // Now attempt to register peer - should fail due to room limit
        let res = manager.add_peer(PeerId::from("p1"), peer);
        assert!(matches!(res, Err(SessionError::RoomLimitExceeded { .. })));
    }

    #[actix::test]
    async fn test_handle_publish_rooms() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);

        // Manager offers 3 rooms
        manager.set_offered_rooms(vec![
            RoomId::from("intentconfig"),
            RoomId::from("memdb"),
            RoomId::from("health"),
        ]);

        // Create a peer (will have empty rooms HashMap for simplicity)
        // In real usage, rooms would be added via add_room()
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let (_tx2, rx2) = tokio::sync::mpsc::channel(100);
        let peer = PeerSession::new_connected(PeerId::from("database"), None, None, tx, rx2)
            .await
            .unwrap();

        // Add peer to manager
        manager.add_peer(PeerId::from("database"), peer).unwrap();

        // Peer offers 2 rooms (none added locally, so intersection empty)
        // Note: This test validates the method works, even though intersection is empty
        // Real usage would have rooms added first
        let result = manager.handle_publish_rooms(
            &PeerId::from("database"),
            vec![RoomId::from("intentconfig"), RoomId::from("admin")],
        );

        // Should fail with empty intersection (no rooms added to peer)
        assert!(matches!(result, Err(SessionError::EmptyIntersection)));
    }

    #[actix::test]
    async fn test_handle_publish_rooms_peer_not_found() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);

        let result = manager
            .handle_publish_rooms(&PeerId::from("unknown"), vec![RoomId::from("intentconfig")]);

        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_peer_joined_rooms() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);

        // Create peer
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let (_tx2, rx2) = tokio::sync::mpsc::channel(100);
        let peer = PeerSession::new_connected(PeerId::from("database"), None, None, tx, rx2)
            .await
            .unwrap();

        manager.add_peer(PeerId::from("database"), peer).unwrap();

        // Before negotiation, no rooms joined
        let joined = manager
            .peer_joined_rooms(&PeerId::from("database"))
            .unwrap();
        assert_eq!(joined.len(), 0);
    }

    #[actix::test]
    async fn test_peer_joined_rooms_peer_not_found() {
        let manager = SessionManager::<MockRole>::new(vec![]);

        let result = manager.peer_joined_rooms(&PeerId::from("unknown"));

        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_is_room_joined_with_peer() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);

        // Create peer
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let (_tx2, rx2) = tokio::sync::mpsc::channel(100);
        let peer = PeerSession::new_connected(PeerId::from("database"), None, None, tx, rx2)
            .await
            .unwrap();

        manager.add_peer(PeerId::from("database"), peer).unwrap();

        // Before negotiation, no rooms joined
        let peer_id = PeerId::from("database");
        assert!(
            !manager
                .is_room_joined_with_peer(&peer_id, &RoomId::from("intentconfig"))
                .unwrap()
        );
        assert!(
            !manager
                .is_room_joined_with_peer(&peer_id, &RoomId::from("admin"))
                .unwrap()
        );
    }

    #[actix::test]
    async fn test_is_room_joined_with_peer_peer_not_found() {
        let manager = SessionManager::<MockRole>::new(vec![]);

        let result = manager
            .is_room_joined_with_peer(&PeerId::from("unknown"), &RoomId::from("intentconfig"));

        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    // --- RoomRegistry Tests ---

    #[test]
    fn test_register_room_handler_success() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);
        let (tx, rx) = mpsc::channel(100);

        let result = manager.register_room_handler("test-room".to_string(), tx, rx);

        assert!(result.is_ok());
        assert_eq!(manager.room_handlers.len(), 1);
    }

    #[test]
    fn test_register_room_handler_duplicate_fails() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);
        let (tx1, rx1) = mpsc::channel(100);
        let (tx2, rx2) = mpsc::channel(100);

        // First registration should succeed
        let result1 = manager.register_room_handler("duplicate".to_string(), tx1, rx1);
        assert!(result1.is_ok());

        // Second registration should fail
        let result2 = manager.register_room_handler("duplicate".to_string(), tx2, rx2);
        assert!(result2.is_err());
        assert!(
            result2
                .unwrap_err()
                .to_string()
                .contains("already registered")
        );
    }

    #[test]
    fn test_register_multiple_rooms() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);

        for i in 0..5 {
            let (tx, rx) = mpsc::channel(100);
            let room_id = format!("room-{}", i);
            let result = manager.register_room_handler(room_id, tx, rx);
            assert!(result.is_ok());
        }

        assert_eq!(manager.room_handlers.len(), 5);
    }

    #[test]
    fn test_register_room_handler_stores_channels() {
        let mut manager = SessionManager::<MockRole>::new(vec![]);
        let (tx, rx) = mpsc::channel(100);

        manager
            .register_room_handler("test-channel".to_string(), tx.clone(), rx)
            .unwrap();

        // Verify channels were stored
        assert!(
            manager
                .room_handlers
                .contains_key(&RoomId::new("test-channel"))
        );
    }
}
