//! RoomManager trait for component-provided room factories.
//!
//! Components implement this trait to provide Room<T> instances per peer.
//! Router orchestrates creation and enforces strict 1:1 room↔component mapping.

use actix::Recipient;
use std::collections::HashSet;
use tokio::sync::mpsc;
use zznet_api::types::{PeerId, Permission, RoomId};

/// Message type for inbound room payloads.
///
/// Room actors receive this message when data arrives from a peer.
/// The payload contains raw bytes that the room actor deserializes.
#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct InboundRoomPayload {
    /// The serialized message payload.
    pub payload: Vec<u8>,
}

/// Alias for the recipient type used by Router ↔ RoomActor wiring.
pub type RoomInboundRecipient = Recipient<InboundRoomPayload>;

/// Error type for room creation failures.
#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    /// Room creation failed with the given reason.
    #[error("Room creation failed: {0}")]
    CreationFailed(String),

    /// The provided permission is invalid for creating this room.
    #[error("Invalid permission for room {room_id}")]
    InvalidPermission {
        /// The room ID that couldn't be created due to permission issues.
        room_id: RoomId,
    },
}

/// Component-provided factory for creating Room<T> instances per peer.
///
/// Components register a RoomManager with the Router at startup.
/// Router calls create_for_peer() when a peer connects, passing a Permission snapshot.
/// Components never query roles at runtime; all auth decisions are precomputed in Permission.
#[async_trait::async_trait]
pub trait RoomManager: Send + Sync {
    /// Returns the set of room IDs this manager can provide.
    ///
    /// Used for collision detection at registration time.
    /// Must be static and unchanging after registration.
    fn managed_rooms(&self) -> HashSet<RoomId>;

    /// Create a room instance for a specific peer.
    ///
    /// Called by Router when a peer connects and offers rooms that intersect with managed_rooms().
    /// Returns Some(room) if this manager owns the room_id, None otherwise.
    /// Errors prevent room creation for this peer (logged and skipped).
    ///
    /// # Arguments
    /// * `peer_id` - Unique peer identifier
    /// * `permission` - Precomputed permission snapshot (no runtime role queries)
    /// * `room_id` - The room to create (must be in managed_rooms())
    /// * `outbound_to_peer` - Channel for sending outbound messages to the peer
    ///
    /// # Returns
    /// * `Ok(Some(room))` - Room created successfully
    /// * `Ok(None)` - This manager doesn't own this room_id
    /// * `Err(CreateError)` - Creation failed (room skipped for this peer)
    async fn create_for_peer(
        &self,
        peer_id: PeerId,
        permission: Permission,
        room_id: &RoomId,
        outbound_to_peer: mpsc::Sender<(RoomId, Vec<u8>)>,
    ) -> Result<Option<RoomInboundRecipient>, CreateError>;
}

/// Actor message for creating a room (if using Actix wrapper).
///
/// Components can implement RoomManagerActor if they prefer actor-based factories.
#[derive(actix::Message)]
#[rtype(result = "Result<Option<RoomInboundRecipient>, CreateError>")]
pub struct CreateRoomForPeer {
    /// The peer ID for which to create the room.
    pub peer_id: PeerId,
    /// The permission snapshot for this peer.
    pub permission: Permission,
    /// The room ID to create.
    pub room_id: RoomId,
    /// The outbound sender to the peer.
    pub outbound_to_peer: mpsc::Sender<(RoomId, Vec<u8>)>,
}

/// Actix actor wrapper for RoomManager (optional convenience).
///
/// Components can implement this instead of RoomManager trait directly.
#[async_trait::async_trait]
pub trait RoomManagerActor:
    actix::Actor<Context = actix::Context<Self>> + actix::Handler<CreateRoomForPeer>
{
    /// Returns the set of room IDs this actor manages.
    fn managed_rooms(&self) -> HashSet<RoomId>;
}
