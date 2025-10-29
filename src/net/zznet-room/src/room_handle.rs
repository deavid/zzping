//! Type-erased room operations shared across the network data plane.
//!
//! `RoomHandle` enables storing heterogeneous `Room<T>` instances in collections by
//! erasing their concrete message types. It is implemented by adapters that bridge
//! component rooms to the network routing layer.

use tokio::sync::mpsc;
use zznet_api::types::{RoomId, SessionError};

/// Trait for type-erased room operations.
///
/// Implementors forward serialized room traffic between component actors and the
/// network transport. The trait intentionally works with raw bytes so that the
/// data plane remains serialization-format agnostic.
pub trait RoomHandle: Send + Sync {
    /// Get the room ID associated with this handle.
    fn room_id(&self) -> &RoomId;

    /// Deliver serialized bytes to the room's inbound channel.
    fn send_message(&mut self, bytes: Vec<u8>) -> Result<(), SessionError>;

    /// Spawn a forwarder task that drains the room's outbound channel and pushes
    /// serialized messages onto the router's peer channel.
    fn spawn_forwarder(&mut self, tx: mpsc::Sender<(RoomId, Vec<u8>)>) -> Result<(), SessionError>;
}
