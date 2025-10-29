use crate::types::{PeerId, PeerIdentity, PeerLifecycleEvent, Role, RoomId, SessionError};
use async_trait::async_trait;
use tokio::sync::broadcast;

/// Control-plane interface for peer state queries and lifecycle events.
///
/// Implementations MUST NOT expose data-plane concerns (channels, routing).
pub trait PeerRegistry: Send + Sync {
    /// Get the authenticated role for a peer, if connected and authenticated.
    fn get_peer_role(&self, peer_id: &PeerId) -> Option<Role>;

    /// Get the full identity information for a peer.
    fn get_peer_identity(&self, peer_id: &PeerId) -> Option<PeerIdentity>;

    /// Get all peer IDs matching a specific role.
    fn peers_with_role(&self, role: &Role) -> Vec<PeerId>;

    /// Get all currently registered peer IDs (connected or not).
    fn peer_ids(&self) -> Vec<PeerId>;

    /// Get count of connected peers.
    fn connected_peer_count(&self) -> usize;

    /// Check if a specific peer is currently connected.
    fn is_peer_connected(&self, peer_id: &PeerId) -> bool;

    /// Subscribe to peer lifecycle events (PeerAdded, PeerConnected, etc).
    fn subscribe_events(&self) -> broadcast::Receiver<PeerLifecycleEvent>;
}

/// Data-plane interface for message routing to peers/rooms.
///
/// Implementations MUST NOT expose control-plane concerns (roles, identity, auth).
#[async_trait]
pub trait MessageRouter: Send + Sync {
    /// Send bytes to a specific peer's room.
    ///
    /// # Errors
    /// - PeerNotFound if peer is not registered
    /// - PeerNotConnected if peer is not in connected state
    /// - RoomNotJoined if peer hasn't negotiated this room
    /// - SendFailed if channel send fails
    async fn send_to_peer(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError>;

    /// Broadcast bytes to multiple peers in a specific room.
    ///
    /// Skips peers that don't have the room joined. Does not fail on partial delivery.
    async fn broadcast_to_peers(
        &self,
        peer_ids: &[PeerId],
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError>;
}
