//! RoomManager trait for component-provided room factories.
//!
//! Components implement this trait to provide Room<T> instances per peer.
//! Router orchestrates creation and enforces strict 1:1 room↔component mapping.

use tokio::sync::mpsc;
use zznet_api::types::{PeerId, Role, RoomId};

// Re-export for backward compatibility
pub use zznet_api::messages::{InboundRoomPayload, RoomInboundRecipient};

/// Actor message for creating a room (if using Actix wrapper).
///
/// Components can implement RoomManagerActor if they prefer actor-based factories.
#[derive(actix::Message)]
#[rtype(result = "Result<Option<RoomInboundRecipient>, ()>")]
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
