use thiserror::Error;
use zznet_api::{PeerId, RoomId};

/// Errors that can occur in session/peer management
#[derive(Debug, Error)]
pub(crate) enum SessionError {
    #[error("Room already exists: {room_id} for peer {peer_id}")]
    /// A room with the same id already exists for the peer.
    RoomAlreadyExists {
        /// The peer id where the room already exists.
        peer_id: PeerId,
        /// The conflicting room id.
        room_id: RoomId,
    },
}
