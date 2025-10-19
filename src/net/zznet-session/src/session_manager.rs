use crate::peer_session::PeerSession;
use crate::room_message_trait::RoomMessageTrait;
use crate::types::{ConnectionState, PeerId, RoomId, SessionError};
use std::collections::HashMap;
use tokio::sync::mpsc;

// NEW: Auth imports
use crate::session_manager_like::SessionManagerLike;
use async_trait::async_trait;
use zznet_api::types::PeerIdentity;
use zznet_auth::ApplicationRole;

const DEFAULT_BROADCAST_TIMEOUT_MS: u64 = 5000;

#[async_trait]
impl<TMsg, TRole> SessionManagerLike<TMsg, TRole> for SessionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static,
    TRole: ApplicationRole,
{
    async fn broadcast_to_room<F>(
        &self,
        room_id: &RoomId,
        message: TMsg,
        filter: F,
        timeout: Option<std::time::Duration>,
    ) -> Vec<(PeerId, Result<(), SessionError>)>
    where
        F: Fn(&TRole) -> bool + Send + Sync + 'static,
    {
        let timeout_duration = timeout.unwrap_or(std::time::Duration::from_millis(
            DEFAULT_BROADCAST_TIMEOUT_MS,
        ));
        <Self>::broadcast_to_room(self, room_id, message, filter, timeout_duration).await
    }

    async fn send_to_room(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        msg: TMsg,
    ) -> Result<(), SessionError> {
        <Self>::send_to_room(self, peer_id, room_id, msg).await
    }

    fn get_peer_role(&self, peer_id: &PeerId) -> Option<TRole> {
        // Return a cloned role if present. This requires TRole: Clone which is
        // enforced on the trait declaration of SessionManagerLike.
        self.get_peer_role_cloned(peer_id)
    }
}

/// Manages all peer sessions for this process
///
/// Generic over TMsg: the application's message enum type that wraps all room messages.
/// Each application defines its own enum (e.g., CollectorMessages, DatabaseMessages).
///
/// 100% typed messages, zero double-serialization. Messages are converted to/from TMsg
/// at the session boundary.
pub struct SessionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    /// All peer sessions
    peers: HashMap<PeerId, PeerSession<TMsg, TRole>>,

    /// Rooms this SessionManager offers
    /// Used during PublishRooms negotiation to compute intersection with peers
    offered_rooms: Vec<RoomId>,

    /// Optional maximum number of peers this manager will accept
    max_peers: Option<usize>,

    /// Optional maximum number of rooms per peer
    max_rooms_per_peer: Option<usize>,
}

impl<TMsg, TRole> SessionManager<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    /// Create a new SessionManager
    ///
    /// `offered_rooms`: List of room IDs this manager offers
    pub fn new(offered_rooms: Vec<RoomId>) -> Self {
        Self {
            peers: HashMap::new(),
            offered_rooms,
            max_peers: None,
            max_rooms_per_peer: None,
        }
    }

    /// Create a new SessionManager with explicit connection limits
    pub fn new_with_limits(
        offered_rooms: Vec<RoomId>,
        max_peers: Option<usize>,
        max_rooms_per_peer: Option<usize>,
    ) -> Self {
        Self {
            peers: HashMap::new(),
            offered_rooms,
            max_peers,
            max_rooms_per_peer,
        }
    }

    /// Add a new peer session (initially disconnected)
    ///
    /// The caller must construct the PeerSession with its rooms already added.
    /// This allows the application to create Room<T> instances with different T types
    /// and type-erase them to Box<dyn RoomHandle<TMsg>> before adding to the session.
    ///
    /// `peer_id`: Unique identifier for this peer
    /// `peer_session`: Pre-configured peer session with rooms
    pub fn add_peer(
        &mut self,
        peer_id: PeerId,
        peer_session: PeerSession<TMsg, TRole>,
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

    /// Connect to a peer with the given channels
    ///
    /// `peer_id`: The peer to connect
    /// `outbound_tx`: Channel to send typed messages (TMsg) to peer
    /// `inbound_rx`: Channel to receive typed messages (TMsg) from peer
    ///
    /// Note: These channels carry TYPED messages (RoomId, TMsg),
    /// NOT bytes. Serialization happens at the transport layer, not here.
    pub fn connect_peer(
        &mut self,
        peer_id: PeerId,
        outbound_tx: mpsc::Sender<(RoomId, TMsg)>,
        inbound_rx: mpsc::Receiver<(RoomId, TMsg)>,
    ) -> Result<(), SessionError> {
        let peer = self
            .peers
            .get_mut(&peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))?;

        peer.connect(outbound_tx, inbound_rx)?;

        tracing::info!("Connected to peer: {}", peer_id);
        Ok(())
    }

    /// Disconnect from a peer
    /// Closes channels and aborts tasks, but keeps peer in map
    pub fn disconnect_peer(&mut self, peer_id: &PeerId) -> Result<(), SessionError> {
        let peer = self
            .peers
            .get_mut(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))?;

        peer.disconnect();

        tracing::info!("Disconnected from peer: {}", peer_id);
        Ok(())
    }

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
    /// The message is the application's enum type (TMsg).
    /// It will be serialized at the transport layer.
    pub async fn send_to_room(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        msg: TMsg,
    ) -> Result<(), SessionError> {
        let peer = self
            .peers
            .get(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))?;

        peer.send_to_room(room_id, msg).await
    }

    /// Broadcast a message to all peers matching a filter in parallel.
    ///
    /// This helper sends `message` to the given `room_id` for every peer whose
    /// role satisfies `filter`. Each send is performed in its own task and the
    /// function returns a vector of per-peer results. A per-send timeout can be
    /// provided to avoid blocking on slow peers.
    pub async fn broadcast_to_room<F>(
        &self,
        room_id: &RoomId,
        message: TMsg,
        filter: F,
        timeout: std::time::Duration,
    ) -> Vec<(PeerId, Result<(), SessionError>)>
    where
        F: Fn(&TRole) -> bool + Send + Sync + 'static,
        TMsg: Clone + Send + 'static,
    {
        use tokio::time::timeout as tokio_timeout;

        let mut handles: Vec<tokio::task::JoinHandle<(PeerId, Result<(), SessionError>)>> =
            Vec::new();
        let mut results: Vec<(PeerId, Result<(), SessionError>)> = Vec::new();

        for peer_id in self.peer_ids() {
            if let Some(role) = self.get_peer_role(&peer_id) {
                if !filter(role) {
                    continue;
                }

                // Try to obtain a cloneable sender for this peer. If none, push an error result.
                if let Some(sender) = self.get_peer_sender(&peer_id) {
                    let room = room_id.clone();
                    let msg_clone = message.clone();
                    let peer_clone = peer_id.clone();

                    let handle = tokio::spawn(async move {
                        // Send via the cloned sender with timeout
                        let send_fut = async {
                            sender
                                .send((room, msg_clone))
                                .await
                                .map_err(|_| SessionError::SendFailed)
                        };

                        match tokio_timeout(timeout, send_fut).await {
                            Ok(r) => (peer_clone, r),
                            Err(_) => (peer_clone, Err(SessionError::SendFailed)),
                        }
                    });

                    handles.push(handle);
                } else {
                    // Peer has no sender (not connected) -> immediate error
                    results.push((
                        peer_id.clone(),
                        Err(SessionError::PeerNotConnected(peer_id.clone())),
                    ));
                }
            }
        }

        // Await spawned tasks and collect their results
        for h in handles {
            match h.await {
                Ok(res) => results.push(res),
                Err(join_err) => {
                    // Task panicked or was cancelled; map to SendFailed with unknown peer
                    tracing::warn!("broadcast task join error: {:?}", join_err);
                }
            }
        }

        results
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
    pub fn get_peer_sender(&self, peer_id: &PeerId) -> Option<mpsc::Sender<(RoomId, TMsg)>> {
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
    ) -> Option<tokio::sync::broadcast::Receiver<(RoomId, TMsg)>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_room_messages::CollectorMessages;
    use tokio::sync::mpsc;
    use zznet_auth::mock::MockRole;

    #[actix::test]
    async fn test_session_manager_new() {
        let offered_rooms = vec![RoomId::from("intentconfig"), RoomId::from("memdb")];
        let manager = SessionManager::<CollectorMessages, MockRole>::new(offered_rooms.clone());

        assert_eq!(manager.offered_rooms(), offered_rooms.as_slice());
        assert_eq!(manager.peer_ids().len(), 0);
        assert_eq!(manager.connected_peer_count(), 0);
    }

    #[actix::test]
    async fn test_add_peer() {
        let mut manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);
        let peer_id = PeerId::from("test_peer");

        // Create empty peer session
        let peer_session = PeerSession::<CollectorMessages, MockRole>::new(peer_id.clone());

        manager.add_peer(peer_id.clone(), peer_session).unwrap();

        assert_eq!(manager.peer_ids(), vec![peer_id.clone()]);
        assert_eq!(
            manager.peer_state(&peer_id),
            Some(ConnectionState::Disconnected)
        );
    }

    #[actix::test]
    async fn test_add_peer_already_exists() {
        let mut manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);
        let peer_id = PeerId::from("test_peer");

        // Add first time
        let peer_session1 = PeerSession::<CollectorMessages, MockRole>::new(peer_id.clone());
        manager.add_peer(peer_id.clone(), peer_session1).unwrap();

        // Try to add again
        let peer_session2 = PeerSession::<CollectorMessages, MockRole>::new(peer_id.clone());
        let result = manager.add_peer(peer_id.clone(), peer_session2);
        assert!(matches!(result, Err(SessionError::PeerAlreadyExists(_))));
    }

    #[actix::test]
    async fn test_remove_peer() {
        let mut manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);
        let peer_id = PeerId::from("test_peer");

        let peer_session = PeerSession::<CollectorMessages, MockRole>::new(peer_id.clone());
        manager.add_peer(peer_id.clone(), peer_session).unwrap();
        assert_eq!(manager.peer_ids().len(), 1);

        manager.remove_peer(&peer_id).unwrap();
        assert_eq!(manager.peer_ids().len(), 0);
    }

    #[actix::test]
    async fn test_remove_peer_not_found() {
        let mut manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);
        let peer_id = PeerId::from("nonexistent_peer");

        let result = manager.remove_peer(&peer_id);
        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_peer_ids() {
        let mut manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);
        let peer_id1 = PeerId::from("peer1");
        let peer_id2 = PeerId::from("peer2");

        let peer_session1 = PeerSession::<CollectorMessages, MockRole>::new(peer_id1.clone());
        let peer_session2 = PeerSession::<CollectorMessages, MockRole>::new(peer_id2.clone());

        manager.add_peer(peer_id1.clone(), peer_session1).unwrap();
        manager.add_peer(peer_id2.clone(), peer_session2).unwrap();

        let mut peer_ids = manager.peer_ids();
        peer_ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));

        assert_eq!(peer_ids, vec![peer_id1, peer_id2]);
    }

    #[actix::test]
    async fn test_offered_rooms() {
        let offered_rooms = vec![RoomId::from("intentconfig"), RoomId::from("memdb")];
        let manager = SessionManager::<CollectorMessages, MockRole>::new(offered_rooms.clone());

        assert_eq!(manager.offered_rooms(), offered_rooms.as_slice());
    }

    #[actix::test]
    async fn test_connect_peer_not_found() {
        let mut manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);
        let peer_id = PeerId::from("nonexistent_peer");
        let (tx, rx) = mpsc::channel(10);

        let result = manager.connect_peer(peer_id, tx, rx);
        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_disconnect_peer_not_found() {
        let mut manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);
        let peer_id = PeerId::from("nonexistent_peer");

        let result = manager.disconnect_peer(&peer_id);
        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_send_to_room_peer_not_found() {
        let manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);
        let peer_id = PeerId::from("nonexistent_peer");
        let room_id = RoomId::from("intentconfig");
        let msg =
            CollectorMessages::IntentConfig(crate::test_room_messages::IntentConfigMessage::Query);

        let result = manager.send_to_room(&peer_id, &room_id, msg).await;
        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_basic_peer_lifecycle() {
        let mut manager =
            SessionManager::<CollectorMessages, MockRole>::new(vec![RoomId::from("memdb")]);

        // Add peer
        let peer_session = PeerSession::<CollectorMessages, MockRole>::new(PeerId::from("peer1"));
        manager
            .add_peer(PeerId::from("peer1"), peer_session)
            .unwrap();

        assert_eq!(
            manager.peer_state(&PeerId::from("peer1")),
            Some(ConnectionState::Disconnected)
        );

        // Connect peer
        let (tx_out, _rx_in) = mpsc::channel(10);
        let (_tx_in, rx_out) = mpsc::channel(10);

        manager
            .connect_peer(PeerId::from("peer1"), tx_out, rx_out)
            .unwrap();

        assert!(manager.is_peer_connected(&PeerId::from("peer1")));

        // Disconnect peer
        manager.disconnect_peer(&PeerId::from("peer1")).unwrap();

        assert!(!manager.is_peer_connected(&PeerId::from("peer1")));
    }

    #[actix::test]
    async fn test_multiple_peers_simultaneously() {
        let mut manager =
            SessionManager::<CollectorMessages, MockRole>::new(vec![RoomId::from("memdb")]);

        // Add three peers
        for peer_id in ["peer1", "peer2", "peer3"] {
            let peer_session =
                PeerSession::<CollectorMessages, MockRole>::new(PeerId::from(peer_id));
            manager
                .add_peer(PeerId::from(peer_id), peer_session)
                .unwrap();
        }

        // Connect all three
        for peer_id in ["peer1", "peer2", "peer3"] {
            let (tx, _rx_in) = mpsc::channel(10);
            let (_tx_in, rx) = mpsc::channel(10);
            manager.connect_peer(PeerId::from(peer_id), tx, rx).unwrap();
        }

        // All should be connected
        assert_eq!(manager.connected_peer_count(), 3);

        // Disconnect peer2
        manager.disconnect_peer(&PeerId::from("peer2")).unwrap();

        // Only 2 connected now
        assert_eq!(manager.connected_peer_count(), 2);
        assert!(manager.is_peer_connected(&PeerId::from("peer1")));
        assert!(!manager.is_peer_connected(&PeerId::from("peer2")));
        assert!(manager.is_peer_connected(&PeerId::from("peer3")));
    }

    // --- New tests for limits ---

    #[actix::test]
    async fn test_peer_limit_exceeded() {
        // Create manager with max_peers = 2
        let mut manager =
            SessionManager::<CollectorMessages, MockRole>::new_with_limits(vec![], Some(2), None);

        // Add two peers - should succeed
        for id in ["peer1", "peer2"] {
            let peer_session = PeerSession::<CollectorMessages, MockRole>::new(PeerId::from(id));
            manager.add_peer(PeerId::from(id), peer_session).unwrap();
        }

        // Third peer should fail
        let peer3 = PeerSession::<CollectorMessages, MockRole>::new(PeerId::from("peer3"));
        let res = manager.add_peer(PeerId::from("peer3"), peer3);
        assert!(matches!(res, Err(SessionError::PeerLimitExceeded { .. })));
    }

    #[actix::test]
    async fn test_room_limit_exceeded_on_add_peer() {
        // Create manager with max_rooms_per_peer = 1
        let mut manager =
            SessionManager::<CollectorMessages, MockRole>::new_with_limits(vec![], None, Some(1));

        // Create peer session and add two simple mock rooms before registering
        let mut peer = PeerSession::<CollectorMessages, MockRole>::new(PeerId::from("p1"));

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

        impl crate::peer_session::RoomHandle<CollectorMessages> for SimpleRoom {
            fn room_id(&self) -> &RoomId {
                &self.id
            }

            fn send_message(&mut self, _msg: CollectorMessages) -> Result<(), SessionError> {
                Ok(())
            }

            fn spawn_forwarder(
                &mut self,
                _tx: mpsc::Sender<(RoomId, CollectorMessages)>,
            ) -> Result<(), SessionError> {
                Ok(())
            }
        }

        // Add two rooms to the peer
        peer.add_room(RoomId::from("r1"), Box::new(SimpleRoom::new("r1")))
            .unwrap();
        peer.add_room(RoomId::from("r2"), Box::new(SimpleRoom::new("r2")))
            .unwrap();

        // Now attempt to register peer - should fail due to room limit
        let res = manager.add_peer(PeerId::from("p1"), peer);
        assert!(matches!(res, Err(SessionError::RoomLimitExceeded { .. })));
    }

    #[actix::test]
    async fn test_handle_publish_rooms() {
        let mut manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);

        // Manager offers 3 rooms
        manager.set_offered_rooms(vec![
            RoomId::from("intentconfig"),
            RoomId::from("memdb"),
            RoomId::from("health"),
        ]);

        // Create a peer (will have empty rooms HashMap for simplicity)
        // In real usage, rooms would be added via add_room()
        let peer = PeerSession::new(PeerId::from("database"));

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
        let mut manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);

        let result = manager
            .handle_publish_rooms(&PeerId::from("unknown"), vec![RoomId::from("intentconfig")]);

        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_peer_joined_rooms() {
        let mut manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);

        // Create peer
        let peer = PeerSession::new(PeerId::from("database"));

        manager.add_peer(PeerId::from("database"), peer).unwrap();

        // Before negotiation, no rooms joined
        let joined = manager
            .peer_joined_rooms(&PeerId::from("database"))
            .unwrap();
        assert_eq!(joined.len(), 0);
    }

    #[actix::test]
    async fn test_peer_joined_rooms_peer_not_found() {
        let manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);

        let result = manager.peer_joined_rooms(&PeerId::from("unknown"));

        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_is_room_joined_with_peer() {
        let mut manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);

        // Create peer
        let peer = PeerSession::new(PeerId::from("database"));

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
        let manager = SessionManager::<CollectorMessages, MockRole>::new(vec![]);

        let result = manager
            .is_room_joined_with_peer(&PeerId::from("unknown"), &RoomId::from("intentconfig"));

        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }
}
