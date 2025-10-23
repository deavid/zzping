//! Room adapter for byte-based messaging
//!
//! This module provides the bridge between Room<T> channels (which work with Vec<u8>)
//! and the SessionManager, enabling PeerSession to store rooms with different
//! component message types in a single collection.
//!
//! **Architecture Change**: Room<T> now handles serialization internally, so this
//! adapter works directly with Vec<u8> instead of typed messages.

use crate::peer_session::RoomHandle;
use crate::types::{RoomId, SessionError};
use tokio::sync::mpsc;

/// Adapter that wraps a Room<T>'s channels and implements RoomHandle
///
/// This enables type erasure: different Room<T> types can be stored as
/// Box<dyn RoomHandle> in PeerSession's HashMap.
///
/// **Key Change**: Works with serialized Vec<u8> instead of typed TMsg.
/// Room<T> handles all serialization/deserialization internally.
///
/// # Example
pub struct RoomAdapter {
    room_id: RoomId,
    // Channel to send inbound messages (serialized) to the room
    inbound_tx: mpsc::Sender<Vec<u8>>,
    // Task that forwards outbound messages
    forwarder_task: Option<tokio::task::JoinHandle<()>>,
}

impl RoomAdapter {
    /// Create a new room adapter
    ///
    /// Takes the room's channels (which work with Vec<u8>) and forwards messages
    /// to/from the peer.
    ///
    /// # Arguments
    ///
    /// - `room_id`: The room identifier (e.g., "intentconfig")
    /// - `inbound_tx`: Channel to send serialized inbound messages to the room
    /// - `outbound_rx`: Channel to receive serialized outbound messages from the room
    /// - `peer_tx`: Channel to send serialized messages to the peer
    pub fn new(
        room_id: RoomId,
        inbound_tx: mpsc::Sender<Vec<u8>>,
        mut outbound_rx: mpsc::Receiver<Vec<u8>>,
        peer_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    ) -> Self {
        // Spawn task to forward outbound messages
        let room_id_clone = room_id.clone();
        let task = tokio::spawn(async move {
            while let Some(bytes) = outbound_rx.recv().await {
                // Forward serialized bytes to peer
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
        // Send serialized bytes to room's inbound channel
        // Use try_send to avoid blocking (room might be processing)
        self.inbound_tx
            .try_send(bytes)
            .map_err(|_| SessionError::SendFailed)?;

        Ok(())
    }

    fn spawn_forwarder(
        &mut self,
        _tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    ) -> Result<(), SessionError> {
        // Forwarder is already spawned in new()
        // This method is called by PeerSession::connect(), but we handle it in constructor
        // Just return Ok if already spawned
        if self.forwarder_task.is_some() {
            Ok(())
        } else {
            Err(SessionError::SendFailed)
        }
    }
}

impl Drop for RoomAdapter {
    fn drop(&mut self) {
        // Abort forwarder task when adapter is dropped
        if let Some(task) = self.forwarder_task.take() {
            task.abort();
        }
    }
}

// Tests will be added in integration tests where we have concrete Message types
// Unit tests here are difficult because IntentConfigMessage doesn't implement actix::Message
// in the test_room_messages module (it's just for serialization testing)
