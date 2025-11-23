use std::collections::HashMap;

use crate::{RoomFactoryRef, error::SessionError};
use zznet_api::types::{PeerId, RoomId};

/// Router - Data Plane for room management and message routing
///
/// **Responsibilities**:
/// - Room negotiation (offered_rooms, PublishRooms handling)
/// - Byte stream routing (send_to_room, broadcast operations)
/// - Room limit enforcement (max_rooms_per_peer)
/// - Room query operations
///
/// **Non-Responsibilities** (handled by ConnectionManager or HelloActor):
/// - Peer state management
/// - Peer identity/roles
/// - Lifecycle events
pub(crate) struct Router {
    /// Registered room factories: RoomId -> Factory (strict 1:1 mapping enforced)
    pub(crate) factories: HashMap<RoomId, RoomFactoryRef>,
}

impl Router {
    /// Create a new Router
    pub(crate) fn new(offered_rooms: Vec<RoomId>) -> Self {
        tracing::info!("Router created with {} offered rooms", offered_rooms.len(),);

        Self {
            factories: HashMap::new(),
        }
    }

    /// Register a room factory with the router.
    ///
    /// Enforces strict 1:1 room↔component mapping; fails on any collision.
    /// Factories provide Room<T> instances per peer at connection time synchronously.
    pub(crate) fn register_manager(
        &mut self,
        factory: RoomFactoryRef,
        rooms: Vec<RoomId>,
    ) -> Result<(), SessionError> {
        for room_id in &rooms {
            if self.factories.contains_key(room_id) {
                return Err(SessionError::RoomAlreadyExists {
                    peer_id: PeerId::from("router"), // dummy, since it's global
                    room_id: room_id.clone(),
                });
            }
        }
        for room_id in rooms {
            self.factories.insert(room_id, factory.clone());
        }
        Ok(())
    }
}
