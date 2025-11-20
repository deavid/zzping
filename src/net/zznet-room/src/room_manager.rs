//! RoomManager trait for component-provided room factories.
//!
//! Components implement this trait to provide Room<T> instances per peer.
//! Router orchestrates creation and enforces strict 1:1 room↔component mapping.

use actix::Recipient;
use std::collections::HashSet;
use tokio::sync::mpsc;
use zznet_api::types::{PeerId, Role, RoomId};

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

/// Actor message for creating a room (if using Actix wrapper).
///
/// Components can implement RoomManagerActor if they prefer actor-based factories.
#[derive(actix::Message)]
#[rtype(result = "Result<Option<RoomInboundRecipient>, CreateError>")]
pub struct CreateRoomForPeer {
    /// The peer ID for which to create the room.
    pub peer_id: PeerId,
    /// The peer's role for authorization.
    pub role: Role,
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
