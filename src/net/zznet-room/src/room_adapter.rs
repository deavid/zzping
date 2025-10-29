//! Room adapter for byte-based messaging.
//!
//! Bridges `Room<T>` channels (which operate on serialized `Vec<u8>` payloads)
//! with the network router so that heterogeneous component rooms can share the
//! same per-peer connection.

use crate::room_handle::RoomHandle;
use tokio::sync::mpsc;
use zznet_api::types::{RoomId, SessionError};

/// Adapter that wraps a `Room<T>`'s channels and implements [`RoomHandle`].
///
/// This enables type erasure: different `Room<T>` types can be stored as
/// `Box<dyn RoomHandle>` in peer connection state.
///
/// The adapter forwards serialized bytes between the component room and the
/// network transport layer.
pub struct RoomAdapter {
    room_id: RoomId,
    inbound_tx: mpsc::Sender<Vec<u8>>,
    forwarder_task: Option<tokio::task::JoinHandle<()>>,
}

impl RoomAdapter {
    /// Create a new room adapter.
    ///
    /// * `room_id` – Identifier of the room (e.g. "intentconfig").
    /// * `inbound_tx` – Channel delivering serialized inbound bytes to the room.
    /// * `outbound_rx` – Channel emitting serialized bytes produced by the room.
    /// * `peer_tx` – Channel feeding serialized bytes into the router's per-peer queue.
    pub fn new(
        room_id: RoomId,
        inbound_tx: mpsc::Sender<Vec<u8>>,
        mut outbound_rx: mpsc::Receiver<Vec<u8>>,
        peer_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    ) -> Self {
        let room_id_clone = room_id.clone();
        let task = tokio::spawn(async move {
            while let Some(bytes) = outbound_rx.recv().await {
                if peer_tx.send((room_id_clone.clone(), bytes)).await.is_err() {
                    tracing::warn!("Room {} outbound channel closed", room_id_clone);
                    break;
                }
            }
            tracing::debug!("Room {} forwarder task stopped", room_id_clone);
        });

        Self {
            room_id,
            inbound_tx,
            forwarder_task: Some(task),
        }
    }
}

impl RoomHandle for RoomAdapter {
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(&mut self, bytes: Vec<u8>) -> Result<(), SessionError> {
        self.inbound_tx
            .try_send(bytes)
            .map_err(|_| SessionError::SendFailed)?;
        Ok(())
    }

    fn spawn_forwarder(
        &mut self,
        _tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    ) -> Result<(), SessionError> {
        if self.forwarder_task.is_some() {
            Ok(())
        } else {
            Err(SessionError::SendFailed)
        }
    }
}

impl Drop for RoomAdapter {
    fn drop(&mut self) {
        if let Some(task) = self.forwarder_task.take() {
            task.abort();
        }
    }
}
