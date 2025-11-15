use thiserror::Error;
use zznet_api::types::PeerId;
use zznet_api::types::RoomId;

/// Errors that can occur in session/peer management
#[derive(Debug, Error)]
pub(crate) enum SessionError {
    #[error("Peer not found: {0}")]
    /// No session exists for the requested peer id.
    PeerNotFound(PeerId),

    #[error("Peer already exists: {0}")]
    /// A peer with the same id was already registered.
    PeerAlreadyExists(PeerId),

    #[error("Room already exists: {room_id} for peer {peer_id}")]
    /// A room with the same id already exists for the peer.
    RoomAlreadyExists {
        /// The peer id where the room already exists.
        peer_id: PeerId,
        /// The conflicting room id.
        room_id: RoomId,
    },
}
