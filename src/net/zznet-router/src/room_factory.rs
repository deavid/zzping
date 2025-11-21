//! RoomFactory trait for synchronous room creation.
//!
//! Components implement this trait to provide synchronous factory methods that create
//! room actors without requiring async/await. This removes the need for a mutex-guarded Router.

use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::mpsc;
use zznet_api::types::{PeerId, Role, RoomId};
use zznet_room::room_manager::RoomInboundRecipient;

/// Factory for creating rooms synchronously on the Router's thread.
///
/// This trait enables components to create room actors without async messaging or mutex contention.
/// The factory runs directly on the Router's thread during peer connection and returns the
/// `RoomInboundRecipient` immediately. The factory can then send fire-and-forget registration
/// messages to managers to track the peer.
///
/// **Thread Safety:** Implementations must be `Send` and `Sync` to safely pass from component context to Router.
///
/// **Synchronous Design:** The `create_room` method uses `&self`, not `&mut self`, to avoid
/// exclusive borrowing requirements. All necessary state should be immutable references or
/// behind interior mutability if needed.
pub trait RoomFactory: Send + Sync {
    /// Create a room for a peer and return the inbound recipient.
    ///
    /// This method runs on the Router's thread during peer connection. It should:
    /// 1. Check permissions (if needed)
    /// 2. Spawn the room and network actors synchronously using `Actor::start`
    /// 3. Send any registration messages (fire-and-forget) to managers
    /// 4. Return the `RoomInboundRecipient` immediately
    ///
    /// # Arguments
    /// - `peer_id`: The ID of the peer connecting
    /// - `role`: The role of the peer (for authorization)
    /// - `room_id`: The ID of the room to create
    /// - `transport_tx`: Direct handle to transport layer for writing raw frames
    ///
    /// # Returns
    /// - `Ok(Some(recipient))` if the room was successfully created
    /// - `Ok(None)` if the peer should not join this room (e.g., authorization denied)
    /// - `Err(...)` if an error occurred during room creation
    fn create_room(
        &self,
        peer_id: PeerId,
        role: Role,
        room_id: RoomId,
        transport_tx: mpsc::Sender<Bytes>,
    ) -> Result<Option<RoomInboundRecipient>, String>;
}

/// Alias for Arc-wrapped RoomFactory for easier passing and storage
pub type RoomFactoryRef = Arc<dyn RoomFactory>;
