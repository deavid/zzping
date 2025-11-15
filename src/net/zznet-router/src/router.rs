use std::collections::HashMap;

use crate::{error::SessionError, peer_channels::PeerChannels};
use zznet_api::types::{PeerId, RoomId};
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
pub(crate) struct Router {
    /// Data-plane channels registered for each peer
    peers: HashMap<PeerId, PeerChannels>,

    /// Registered room managers: RoomId -> Manager (strict 1:1 mapping enforced)
    pub(crate) managers: HashMap<RoomId, std::sync::Arc<dyn RoomManager + Send + Sync>>,
}

impl Router {
    /// Create a new Router
    pub(crate) fn new(offered_rooms: Vec<RoomId>) -> Self {
        tracing::info!("Router created with {} offered rooms", offered_rooms.len(),);

        Self {
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
    pub(crate) fn register_manager(
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

    /// Register a peer's channel set with the router.
    pub(crate) fn register_peer(&mut self, channels: PeerChannels) -> Result<(), SessionError> {
        let peer_id = channels.peer_id.clone();

        if self.peers.contains_key(&peer_id) {
            return Err(SessionError::PeerAlreadyExists(peer_id));
        }

        self.peers.insert(peer_id, channels);
        Ok(())
    }

    /// Disconnect but retain the peer's registered channels.
    pub(crate) fn disconnect_peer(&mut self, peer_id: &PeerId) -> Result<(), SessionError> {
        let peer = self
            .peers
            .get_mut(peer_id)
            .ok_or_else(|| SessionError::PeerNotFound(peer_id.clone()))?;
        peer.disconnect();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let mut router = Router::new(vec![]);

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

        let mut router = Router::new(vec![]);

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
