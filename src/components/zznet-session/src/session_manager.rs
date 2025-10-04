use crate::peer_session::PeerSession;
use crate::room_message_trait::RoomMessageTrait;
use crate::types::{ConnectionState, PeerId, RoomId, SessionError};
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Manages all peer sessions for this process
///
/// Generic over TMsg: the application's message enum type that wraps all room messages.
/// Each application defines its own enum (e.g., CollectorMessages, DatabaseMessages).
///
/// 100% typed messages, zero double-serialization. Messages are converted to/from TMsg
/// at the session boundary.
pub struct SessionManager<TMsg>
where
    TMsg: RoomMessageTrait,
{
    /// All peer sessions
    peers: HashMap<PeerId, PeerSession<TMsg>>,

    /// Rooms this SessionManager offers
    /// Used during PublishRooms negotiation to compute intersection with peers
    offered_rooms: Vec<RoomId>,
}

impl<TMsg> SessionManager<TMsg>
where
    TMsg: RoomMessageTrait,
{
    /// Create a new SessionManager
    ///
    /// `offered_rooms`: List of room IDs this manager offers
    pub fn new(offered_rooms: Vec<RoomId>) -> Self {
        Self {
            peers: HashMap::new(),
            offered_rooms,
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
        peer_session: PeerSession<TMsg>,
    ) -> Result<(), SessionError> {
        if self.peers.contains_key(&peer_id) {
            return Err(SessionError::PeerAlreadyExists(peer_id));
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

    /// Get list of all peer IDs
    pub fn peer_ids(&self) -> Vec<PeerId> {
        self.peers.keys().cloned().collect()
    }

    /// Get number of connected peers
    pub fn connected_peer_count(&self) -> usize {
        self.peers.values().filter(|p| p.is_connected()).count()
    }

    /// Get the list of offered rooms
    pub fn offered_rooms(&self) -> &[RoomId] {
        &self.offered_rooms
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
    #[actix::test]
    async fn test_session_manager_new() {
        let offered_rooms = vec![RoomId::from("intentconfig"), RoomId::from("memdb")];
        let manager = SessionManager::<CollectorMessages>::new(offered_rooms.clone());

        assert_eq!(manager.offered_rooms(), offered_rooms.as_slice());
        assert_eq!(manager.peer_ids().len(), 0);
        assert_eq!(manager.connected_peer_count(), 0);
    }

    #[actix::test]
    async fn test_add_peer() {
        let mut manager = SessionManager::<CollectorMessages>::new(vec![]);
        let peer_id = PeerId::from("test_peer");

        // Create empty peer session
        let peer_session = PeerSession::<CollectorMessages>::new(peer_id.clone());

        manager.add_peer(peer_id.clone(), peer_session).unwrap();

        assert_eq!(manager.peer_ids(), vec![peer_id.clone()]);
        assert_eq!(
            manager.peer_state(&peer_id),
            Some(ConnectionState::Disconnected)
        );
    }

    #[actix::test]
    async fn test_add_peer_already_exists() {
        let mut manager = SessionManager::<CollectorMessages>::new(vec![]);
        let peer_id = PeerId::from("test_peer");

        // Add first time
        let peer_session1 = PeerSession::<CollectorMessages>::new(peer_id.clone());
        manager.add_peer(peer_id.clone(), peer_session1).unwrap();

        // Try to add again
        let peer_session2 = PeerSession::<CollectorMessages>::new(peer_id.clone());
        let result = manager.add_peer(peer_id.clone(), peer_session2);
        assert!(matches!(result, Err(SessionError::PeerAlreadyExists(_))));
    }

    #[actix::test]
    async fn test_remove_peer() {
        let mut manager = SessionManager::<CollectorMessages>::new(vec![]);
        let peer_id = PeerId::from("test_peer");

        let peer_session = PeerSession::<CollectorMessages>::new(peer_id.clone());
        manager.add_peer(peer_id.clone(), peer_session).unwrap();
        assert_eq!(manager.peer_ids().len(), 1);

        manager.remove_peer(&peer_id).unwrap();
        assert_eq!(manager.peer_ids().len(), 0);
    }

    #[actix::test]
    async fn test_remove_peer_not_found() {
        let mut manager = SessionManager::<CollectorMessages>::new(vec![]);
        let peer_id = PeerId::from("nonexistent_peer");

        let result = manager.remove_peer(&peer_id);
        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_peer_ids() {
        let mut manager = SessionManager::<CollectorMessages>::new(vec![]);
        let peer_id1 = PeerId::from("peer1");
        let peer_id2 = PeerId::from("peer2");

        let peer_session1 = PeerSession::<CollectorMessages>::new(peer_id1.clone());
        let peer_session2 = PeerSession::<CollectorMessages>::new(peer_id2.clone());

        manager.add_peer(peer_id1.clone(), peer_session1).unwrap();
        manager.add_peer(peer_id2.clone(), peer_session2).unwrap();

        let mut peer_ids = manager.peer_ids();
        peer_ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));

        assert_eq!(peer_ids, vec![peer_id1, peer_id2]);
    }

    #[actix::test]
    async fn test_offered_rooms() {
        let offered_rooms = vec![RoomId::from("intentconfig"), RoomId::from("memdb")];
        let manager = SessionManager::<CollectorMessages>::new(offered_rooms.clone());

        assert_eq!(manager.offered_rooms(), offered_rooms.as_slice());
    }

    #[actix::test]
    async fn test_connect_peer_not_found() {
        let mut manager = SessionManager::<CollectorMessages>::new(vec![]);
        let peer_id = PeerId::from("nonexistent_peer");
        let (tx, rx) = mpsc::channel(10);

        let result = manager.connect_peer(peer_id, tx, rx);
        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_disconnect_peer_not_found() {
        let mut manager = SessionManager::<CollectorMessages>::new(vec![]);
        let peer_id = PeerId::from("nonexistent_peer");

        let result = manager.disconnect_peer(&peer_id);
        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_send_to_room_peer_not_found() {
        let manager = SessionManager::<CollectorMessages>::new(vec![]);
        let peer_id = PeerId::from("nonexistent_peer");
        let room_id = RoomId::from("intentconfig");
        let msg =
            CollectorMessages::IntentConfig(crate::test_room_messages::IntentConfigMessage::Query);

        let result = manager.send_to_room(&peer_id, &room_id, msg).await;
        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_basic_peer_lifecycle() {
        let mut manager = SessionManager::<CollectorMessages>::new(vec![RoomId::from("memdb")]);

        // Add peer
        let peer_session = PeerSession::<CollectorMessages>::new(PeerId::from("peer1"));
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
        let mut manager = SessionManager::<CollectorMessages>::new(vec![RoomId::from("memdb")]);

        // Add three peers
        for peer_id in ["peer1", "peer2", "peer3"] {
            let peer_session = PeerSession::<CollectorMessages>::new(PeerId::from(peer_id));
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

    // Note: Full integration tests with Room<T> instances are in integration_tests.rs
    // These tests focus on SessionManager's core peer management functionality.

    // --- Phase 8: Room Negotiation Tests ---

    #[actix::test]
    async fn test_set_offered_rooms() {
        let mut manager = SessionManager::<CollectorMessages>::new(vec![]);

        // Initially empty
        assert_eq!(manager.offered_rooms().len(), 0);

        // Set rooms
        let rooms = vec![
            RoomId::from("intentconfig"),
            RoomId::from("memdb"),
            RoomId::from("health"),
        ];
        manager.set_offered_rooms(rooms.clone());

        // Should be set
        assert_eq!(manager.offered_rooms(), rooms.as_slice());

        // Can update
        let new_rooms = vec![RoomId::from("intentconfig"), RoomId::from("metrics")];
        manager.set_offered_rooms(new_rooms.clone());
        assert_eq!(manager.offered_rooms(), new_rooms.as_slice());
    }

    #[actix::test]
    async fn test_handle_publish_rooms() {
        let mut manager = SessionManager::<CollectorMessages>::new(vec![]);

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
        let mut manager = SessionManager::<CollectorMessages>::new(vec![]);

        let result = manager
            .handle_publish_rooms(&PeerId::from("unknown"), vec![RoomId::from("intentconfig")]);

        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_peer_joined_rooms() {
        let mut manager = SessionManager::<CollectorMessages>::new(vec![]);

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
        let manager = SessionManager::<CollectorMessages>::new(vec![]);

        let result = manager.peer_joined_rooms(&PeerId::from("unknown"));

        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }

    #[actix::test]
    async fn test_is_room_joined_with_peer() {
        let mut manager = SessionManager::<CollectorMessages>::new(vec![]);

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
        let manager = SessionManager::<CollectorMessages>::new(vec![]);

        let result = manager
            .is_room_joined_with_peer(&PeerId::from("unknown"), &RoomId::from("intentconfig"));

        assert!(matches!(result, Err(SessionError::PeerNotFound(_))));
    }
}
