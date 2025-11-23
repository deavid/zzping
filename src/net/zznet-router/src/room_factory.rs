//! Synchronous factory trait for creating room actors on the Router thread.

use std::sync::Arc;
use tokio::sync::mpsc;
use zznet_api::types::{PeerId, Role, RoomId, TransportFrame};
use zznet_room::room_manager::RoomInboundRecipient;

/// Creates room actors synchronously during peer connection on the Router thread.
pub trait RoomFactory: Send + Sync {
    /// Create a room actor for a connecting peer and return its inbound recipient.
    fn create_room(
        &self,
        peer_id: PeerId,
        role: Role,
        room_id: RoomId,
        transport_tx: mpsc::Sender<TransportFrame>,
    ) -> Result<Option<RoomInboundRecipient>, String>;
}

/// Alias for an `Arc`-wrapped `RoomFactory`.
pub type RoomFactoryRef = Arc<dyn RoomFactory>;
