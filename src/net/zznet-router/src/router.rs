use std::collections::HashMap;

use crate::{error::SessionError, peer_channels::PeerChannels};
use zznet_api::types::{PeerId, RoomId};
use zznet_room::room_manager::CreateRoomForPeer;

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
    pub(crate) managers: HashMap<RoomId, actix::Recipient<CreateRoomForPeer>>,
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
        manager: actix::Recipient<CreateRoomForPeer>,
        rooms: Vec<RoomId>,
    ) -> Result<(), SessionError> {
        for room_id in &rooms {
            if self.managers.contains_key(room_id) {
                return Err(SessionError::RoomAlreadyExists {
                    peer_id: PeerId::from("router"), // dummy, since it's global
                    room_id: room_id.clone(),
                });
            }
        }
        for room_id in rooms {
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
    use actix::prelude::*;
    use zznet_room::room_manager::{CreateRoomForPeer, RoomInboundRecipient};

    // Mock actor for testing
    struct MockManager;

    impl Actor for MockManager {
        type Context = Context<Self>;
    }

    impl Handler<CreateRoomForPeer> for MockManager {
        type Result = Result<Option<RoomInboundRecipient>, ()>;

        fn handle(&mut self, _msg: CreateRoomForPeer, _ctx: &mut Context<Self>) -> Self::Result {
            Ok(None)
        }
    }

    #[actix::test]
    async fn test_register_manager_collision() {
        let mut router = Router::new(vec![]);

        // Start a mock manager actor
        let mock_addr = MockManager.start();

        let rooms1 = vec![RoomId::from("room1")];
        assert!(
            router
                .register_manager(mock_addr.clone().recipient(), rooms1)
                .is_ok()
        );

        let rooms2 = vec![RoomId::from("room1")]; // collision
        let result = router.register_manager(mock_addr.recipient(), rooms2);
        assert!(matches!(
            result,
            Err(SessionError::RoomAlreadyExists { .. })
        ));
    }

    #[actix::test]
    async fn test_registered_rooms() {
        let mut router = Router::new(vec![]);

        // Start a mock manager actor
        let mock_addr = MockManager.start();

        let rooms = vec![RoomId::from("room1"), RoomId::from("room2")];
        router
            .register_manager(mock_addr.recipient(), rooms)
            .unwrap();

        let registered: Vec<RoomId> = router.managers.keys().cloned().collect();
        assert_eq!(registered.len(), 2);
        assert!(registered.contains(&RoomId::from("room1")));
        assert!(registered.contains(&RoomId::from("room2")));
    }

    // Note: Full integration tests for handle_publish_rooms, send_to_room, and
    // broadcast_to_role require PeerSession instances and are better tested
    // at the SessionManager level during integration testing.
}
