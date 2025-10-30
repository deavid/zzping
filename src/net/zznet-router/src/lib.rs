//! zznet-router
//!
//! Data-plane router: manages per-peer channels, room membership/negotiation, and byte routing.
//!
//! Allowed responsibilities:
//! - Channel registration (peer -> sender/receiver)
//! - Room membership and negotiation (PublishRooms)
//! - Byte routing to room handlers
//! - Inbound/outbound broadcast/send operations
//!
//! NOT allowed:
//! - Role queries or auth checks
//! - Peer identity or lifecycle state
//! - Business logic
//!
//! This crate implements the `MessageRouter` trait from `zznet-api` and must not import
//! business/auth types such as `Role` or `PeerIdentity`.

use std::collections::HashMap;
use tokio::sync::{broadcast, mpsc};

mod actor;
mod peer_channels;

pub use actor::{
    HandlePublishRooms, IsRoomJoined, OnPeerConnected, OnPeerDisconnected, PeerJoinedRooms,
    PeerSender, RegisterManager, RouterActor, SubscribePeerInbound,
};
pub use peer_channels::PeerChannels;

// Re-export canonical types from zznet-api
pub use zznet_api::types::{PeerChannels as PeerChannelsTrait, PeerId, RoomId, SessionError};

// Import room manager types
use zznet_room::room_manager::RoomManager;

/// Router - Data Plane for room management and message routing
///
/// **Responsibilities**:
/// - Room negotiation (offered_rooms, PublishRooms handling)
/// - Byte stream routing (send_to_room, broadcast operations)
/// - Room limit enforcement (max_rooms_per_peer)
/// - Room query operations
///
/// **Non-Responsibilities** (handled by PeerManager):
/// - Peer state management
/// - Peer identity/roles
/// - Lifecycle events
pub struct Router {
    /// Rooms this Router offers for negotiation
    /// Used during PublishRooms to compute intersection with peers
    offered_rooms: Vec<RoomId>,

    /// Optional maximum number of rooms per peer
    max_rooms_per_peer: Option<usize>,

    /// Data-plane channels registered for each peer
    peers: HashMap<PeerId, PeerChannels>,

    /// Registered room managers: RoomId -> Manager (strict 1:1 mapping enforced)
    managers: HashMap<RoomId, std::sync::Arc<dyn RoomManager + Send + Sync>>,
}

impl Router {
    /// Create a new Router
    ///
    /// # Arguments
    /// * `offered_rooms` - List of room IDs this router offers
    /// * `max_rooms_per_peer` - Optional limit on rooms per peer
    ///
    /// # Example
    /// ```
    /// use zznet_router::Router;
    /// use zznet_api::types::RoomId;
    ///
    /// let rooms = vec![RoomId::from("chat"), RoomId::from("data")];
    /// let router = Router::new(rooms, None);
    /// ```
    pub fn new(offered_rooms: Vec<RoomId>, max_rooms_per_peer: Option<usize>) -> Self {
        tracing::info!(
            "Router created with {} offered rooms, max_rooms_per_peer = {:?}",
            offered_rooms.len(),
            max_rooms_per_peer
        );

        Self {
            offered_rooms,
            max_rooms_per_peer,
            peers: HashMap::new(),
            managers: HashMap::new(),
        }
    }

    /// Get the list of offered rooms
    pub fn offered_rooms(&self) -> &[RoomId] {
        &self.offered_rooms
    }

    /// Set the offered rooms (for dynamic reconfiguration)
    pub fn set_offered_rooms(&mut self, rooms: Vec<RoomId>) {
        self.offered_rooms = rooms;
        tracing::debug!("Offered rooms updated: {} rooms", self.offered_rooms.len());
    }

    /// Get the configured max_rooms_per_peer limit
    pub fn max_rooms_per_peer(&self) -> Option<usize> {
        self.max_rooms_per_peer
    }

    /// Register a room manager with the router.
    ///
    /// Enforces strict 1:1 room↔component mapping; fails on any collision.
    /// Managers provide Room<T> instances per peer at connection time.
    ///
    /// # Errors
    /// - `SessionError::RoomAlreadyExists` if any managed room is already registered
    pub fn register_manager(
        &mut self,
        manager: std::sync::Arc<dyn RoomManager + Send + Sync>,
    ) -> Result<(), SessionError> {
        let managed_rooms = manager.managed_rooms();
        for room_id in &managed_rooms {
            if self.managers.contains_key(room_id) {
                return Err(SessionError::RoomAlreadyExists {
                    peer_id: PeerId::from("router"), // dummy, since it's global
                    room_id: room_id.clone(),
                });
            }
        }
        for room_id in managed_rooms {
            self.managers.insert(room_id, manager.clone());
        }
        Ok(())
    }

    /// Get the list of registered rooms (union of all managers' rooms)
    pub fn registered_rooms(&self) -> Vec<RoomId> {
        self.managers.keys().cloned().collect()
    }

    /// Validate room count against max_rooms_per_peer
    ///
    /// Returns error if room_count exceeds the limit
    pub fn validate_room_count(
        &self,
        peer_id: &PeerId,
        room_count: usize,
    ) -> Result<(), SessionError> {
        if let Some(max_rooms) = self.max_rooms_per_peer
            && room_count > max_rooms
        {
            return Err(SessionError::RoomLimitExceeded {
                peer_id: peer_id.clone(),
                max: max_rooms,
            });
        }
        Ok(())
    }

    /// Register a peer's channel set with the router.
    pub fn register_peer(&mut self, channels: PeerChannels) -> Result<(), SessionError> {
        let peer_id = channels.peer_id().clone();

        if self.peers.contains_key(&peer_id) {
            return Err(SessionError::PeerAlreadyExists(peer_id));
        }

        self.peers.insert(peer_id, channels);
        Ok(())
    }

    /// Remove a peer's channel set from the router.
    pub fn remove_peer(&mut self, peer_id: &PeerId) -> Result<(), SessionError> {
        self.peers
            .remove(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))?;
        Ok(())
    }

    /// Disconnect but retain the peer's registered channels.
    pub fn disconnect_peer(&mut self, peer_id: &PeerId) -> Result<(), SessionError> {
        let peer = self.peer_mut(peer_id)?;
        peer.disconnect();
        Ok(())
    }

    fn peer_mut(&mut self, peer_id: &PeerId) -> Result<&mut PeerChannels, SessionError> {
        self.peers
            .get_mut(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))
    }

    fn peer(&self, peer_id: &PeerId) -> Result<&PeerChannels, SessionError> {
        self.peers
            .get(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))
    }

    /// Handle PublishRooms from a peer
    ///
    /// Delegates to PeerSession for actual negotiation, validates room limits
    ///
    /// # Errors
    /// - `RoomLimitExceeded` if peer tries to join too many rooms
    pub async fn handle_publish_rooms(
        &mut self,
        peer_id: &PeerId,
        peer_rooms: Vec<RoomId>,
    ) -> Result<Vec<RoomId>, SessionError> {
        let offered = self.offered_rooms.clone();
        let peer = self.peer_mut(peer_id)?;
        peer.handle_publish_rooms(&offered, peer_rooms).await?;
        let joined_rooms = peer.joined_rooms().await;

        // Validate room limit after negotiation
        self.validate_room_count(peer_id, joined_rooms.len())?;

        tracing::debug!(
            "Room negotiation complete for {}: {} rooms joined",
            peer_id,
            joined_rooms.len()
        );

        Ok(joined_rooms)
    }

    /// Send message to a specific peer's room
    ///
    /// # Errors
    /// - `PeerNotConnected` if peer not in connected state
    /// - `RoomNotJoined` if room not negotiated with peer
    /// - `SendFailed` if channel send fails
    pub async fn send_to_room(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError> {
        let peer = self.peer(peer_id)?;

        if !peer.is_room_joined(room_id).await {
            return Err(SessionError::RoomNotJoined(room_id.clone()));
        }

        peer.send_raw_to_room(room_id, bytes).await
    }

    /// Broadcast message to a set of peers already filtered by the caller.
    pub async fn broadcast_to_peers(
        &self,
        peers: &[PeerId],
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError> {
        for peer_id in peers {
            if let Ok(peer) = self.peer(peer_id)
                && peer.is_room_joined(room_id).await
            {
                peer.send_raw_to_room(room_id, bytes.clone()).await?;
            }
        }

        Ok(())
    }

    /// Get the rooms joined with a specific peer
    pub async fn peer_joined_rooms(&self, peer_id: &PeerId) -> Result<Vec<RoomId>, SessionError> {
        Ok(self.peer(peer_id)?.joined_rooms().await)
    }

    /// Check if a specific room is joined with a peer
    pub async fn is_room_joined(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
    ) -> Result<bool, SessionError> {
        Ok(self.peer(peer_id)?.is_room_joined(room_id).await)
    }

    /// Clone the outbound sender if the peer is connected.
    pub fn peer_sender(
        &self,
        peer_id: &PeerId,
    ) -> Result<Option<mpsc::Sender<(RoomId, Vec<u8>)>>, SessionError> {
        Ok(self.peer(peer_id)?.outbound_sender())
    }

    /// Subscribe to inbound messages from the peer if connected.
    pub fn subscribe_peer_inbound(
        &self,
        peer_id: &PeerId,
    ) -> Result<Option<broadcast::Receiver<(RoomId, Vec<u8>)>>, SessionError> {
        Ok(self.peer(peer_id)?.subscribe_inbound())
    }
}

#[async_trait::async_trait]
impl zznet_api::traits::MessageRouter for Router {
    async fn send_to_peer(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError> {
        self.send_to_room(peer_id, room_id, bytes).await
    }

    async fn broadcast_to_peers(
        &self,
        peer_ids: &[PeerId],
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError> {
        self.broadcast_to_peers(peer_ids, room_id, bytes).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_router_offered_rooms() {
        let offered = vec![RoomId::from("room1"), RoomId::from("room2")];
        let router = Router::new(offered.clone(), None);

        assert_eq!(router.offered_rooms(), offered.as_slice());
    }

    #[tokio::test]
    async fn test_set_offered_rooms() {
        let mut router = Router::new(vec![], None);

        let new_rooms = vec![RoomId::from("roomA"), RoomId::from("roomB")];
        router.set_offered_rooms(new_rooms.clone());

        assert_eq!(router.offered_rooms(), new_rooms.as_slice());
    }

    #[tokio::test]
    async fn test_validate_room_count() {
        let router = Router::new(vec![], Some(2));
        let peer_id = PeerId::from("test-peer");

        // 1 room - OK
        assert!(router.validate_room_count(&peer_id, 1).is_ok());

        // 2 rooms - OK
        assert!(router.validate_room_count(&peer_id, 2).is_ok());

        // 3 rooms - Should fail
        let result = router.validate_room_count(&peer_id, 3);
        assert!(matches!(
            result,
            Err(SessionError::RoomLimitExceeded { max: 2, .. })
        ));
    }

    #[tokio::test]
    async fn test_max_rooms_per_peer() {
        let router_unlimited = Router::new(vec![], None);
        assert_eq!(router_unlimited.max_rooms_per_peer(), None);

        let router_limited = Router::new(vec![], Some(5));
        assert_eq!(router_limited.max_rooms_per_peer(), Some(5));
    }

    #[tokio::test]
    async fn test_register_manager_collision() {
        use std::collections::HashSet;
        use zznet_api::types::{PeerId, Permission, RoomId};
        use zznet_room::room_manager::{CreateError, RoomManager};

        struct MockManager {
            rooms: HashSet<RoomId>,
        }

        #[async_trait::async_trait]
        impl RoomManager for MockManager {
            fn managed_rooms(&self) -> HashSet<RoomId> {
                self.rooms.clone()
            }

            async fn create_for_peer(
                &self,
                _peer_id: PeerId,
                _permission: Permission,
                _room_id: &RoomId,
                _outbound_to_peer: tokio::sync::mpsc::Sender<(RoomId, Vec<u8>)>,
            ) -> Result<
                Option<actix::Recipient<zznet_room::room_manager::InboundRoomPayload>>,
                CreateError,
            > {
                Ok(None)
            }
        }

        let mut router = Router::new(vec![], None);

        let manager1 = std::sync::Arc::new(MockManager {
            rooms: HashSet::from([RoomId::from("room1")]),
        });
        assert!(router.register_manager(manager1).is_ok());

        let manager2 = std::sync::Arc::new(MockManager {
            rooms: HashSet::from([RoomId::from("room1")]), // collision
        });
        let result = router.register_manager(manager2);
        assert!(matches!(
            result,
            Err(SessionError::RoomAlreadyExists { .. })
        ));
    }

    #[tokio::test]
    async fn test_registered_rooms() {
        use std::collections::HashSet;
        use zznet_api::types::{PeerId, Permission, RoomId};
        use zznet_room::room_manager::{CreateError, RoomManager};

        struct MockManager {
            rooms: HashSet<RoomId>,
        }

        #[async_trait::async_trait]
        impl RoomManager for MockManager {
            fn managed_rooms(&self) -> HashSet<RoomId> {
                self.rooms.clone()
            }

            async fn create_for_peer(
                &self,
                _peer_id: PeerId,
                _permission: Permission,
                _room_id: &RoomId,
                _outbound_to_peer: tokio::sync::mpsc::Sender<(RoomId, Vec<u8>)>,
            ) -> Result<
                Option<actix::Recipient<zznet_room::room_manager::InboundRoomPayload>>,
                CreateError,
            > {
                Ok(None)
            }
        }

        let mut router = Router::new(vec![], None);

        let manager1 = std::sync::Arc::new(MockManager {
            rooms: HashSet::from([RoomId::from("room1"), RoomId::from("room2")]),
        });
        router.register_manager(manager1).unwrap();

        let registered = router.registered_rooms();
        assert_eq!(registered.len(), 2);
        assert!(registered.contains(&RoomId::from("room1")));
        assert!(registered.contains(&RoomId::from("room2")));
    }

    // Note: Full integration tests for handle_publish_rooms, send_to_room, and
    // broadcast_to_role require PeerSession instances and are better tested
    // at the SessionManager level during integration testing.
}
