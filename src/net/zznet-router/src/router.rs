use std::collections::HashMap;
use tokio::sync::{broadcast, mpsc};

use crate::peer_channels::PeerChannels;
use zznet_api::types::{PeerChannels as PeerChannelsTrait, PeerId, RoomId, SessionError};
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
    // FIXME: DEPRECATED - REMOVE rooms per peer.
    max_rooms_per_peer: Option<usize>,

    /// Data-plane channels registered for each peer
    peers: HashMap<PeerId, PeerChannels>,

    /// Registered room managers: RoomId -> Manager (strict 1:1 mapping enforced)
    pub(crate) managers: HashMap<RoomId, std::sync::Arc<dyn RoomManager + Send + Sync>>,
}

// Type aliases to reduce signature complexity in public methods
type OutboundSender = mpsc::Sender<(RoomId, Vec<u8>)>;
type InboundReceiver = broadcast::Receiver<(RoomId, Vec<u8>)>;

impl Router {
    /// Create a new Router
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

    // Registered rooms can be obtained from `self.managers` directly.

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
    /// - `SessionError::RoomLimitExceeded` if peer tries to join too many rooms
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

    // NOTE: Direct send/broadcast helpers were removed. Callers should obtain
    // the peer outbound sender via `peer_sender()` and perform sends directly.

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
    pub fn peer_sender(&self, peer_id: &PeerId) -> Result<Option<OutboundSender>, SessionError> {
        Ok(self.peer(peer_id)?.outbound_sender())
    }

    /// Subscribe to inbound messages from the peer if connected.
    pub fn subscribe_peer_inbound(
        &self,
        peer_id: &PeerId,
    ) -> Result<Option<InboundReceiver>, SessionError> {
        Ok(self.peer(peer_id)?.subscribe_inbound())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_router_offered_rooms() {
        let offered = vec![RoomId::from("room1"), RoomId::from("room2")];
        let router = Router::new(offered.clone(), None);

        assert_eq!(router.offered_rooms.as_slice(), offered.as_slice());
    }

    #[tokio::test]
    async fn test_set_offered_rooms() {
        let mut router = Router::new(vec![], None);

        let new_rooms = vec![RoomId::from("roomA"), RoomId::from("roomB")];
        router.offered_rooms = new_rooms.clone();

        assert_eq!(router.offered_rooms.as_slice(), new_rooms.as_slice());
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
        assert_eq!(router_unlimited.max_rooms_per_peer, None);

        let router_limited = Router::new(vec![], Some(5));
        assert_eq!(router_limited.max_rooms_per_peer, Some(5));
    }

    #[tokio::test]
    async fn test_register_manager_collision() {
        use std::collections::HashSet;
        use zznet_api::types::{PeerId, Role, RoomId};
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
                _role: Role,
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
        use zznet_api::types::{PeerId, Role, RoomId};
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
                _role: Role,
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

        let registered: Vec<RoomId> = router.managers.keys().cloned().collect();
        assert_eq!(registered.len(), 2);
        assert!(registered.contains(&RoomId::from("room1")));
        assert!(registered.contains(&RoomId::from("room2")));
    }

    // Note: Full integration tests for handle_publish_rooms, send_to_room, and
    // broadcast_to_role require PeerSession instances and are better tested
    // at the SessionManager level during integration testing.
}
