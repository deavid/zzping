//! Test utilities for the zzping project.
//!
//! This crate provides small helpers used by unit and integration tests in the
//! workspace. The helpers create in-memory session managers, message capture
//! channels, and minimal room handles so tests can exercise session logic
//! without real network or IO dependencies.
//!
//! These utilities are intentionally lightweight and synchronous-friendly so
//! they can be composed easily in tests.

use std::time::Duration;
use tokio::sync::mpsc;
use zznet_api::types::RoomId;

/// Return type for MessageCapture creation that bundles the capture handle with connection channels
pub struct MessageCaptureChannels {
    /// The capture handle used to receive messages from the connected peer.
    pub capture: MessageCapture,
    /// Sender side for outbound messages into the peer under test.
    pub tx_out: mpsc::Sender<(RoomId, Vec<u8>)>,
    /// Receiver side for inbound messages that will be delivered to the peer.
    pub rx_in: mpsc::Receiver<(RoomId, Vec<u8>)>,
}

/// Message capture helper that sets up channels and provides a way to wait for messages.
/// This reduces boilerplate for the common pattern of:
/// 1. Creating channels
/// 2. Connecting peer
/// 3. Waiting for messages with timeout
pub struct MessageCapture {
    /// Receiver for captured messages. Tests can `.recv()` on this to observe
    /// messages published by the peer under test.
    pub rx: mpsc::Receiver<(RoomId, Vec<u8>)>,
    // Internal sender kept alive so the channel remains open while the
    // capture exists.
    _tx: mpsc::Sender<(RoomId, Vec<u8>)>, // Keep alive
}

impl MessageCapture
where
    Vec<u8>: Send + 'static,
{
    /// Wait for the next message with a timeout
    pub async fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<(RoomId, Vec<u8>)>, tokio::time::error::Elapsed> {
        tokio::time::timeout(timeout, self.rx.recv()).await
    }

    /// Wait for the next message with default timeout (500ms)
    pub async fn recv_default_timeout(
        &mut self,
    ) -> Result<Option<(RoomId, Vec<u8>)>, tokio::time::error::Elapsed> {
        self.recv_timeout(Duration::from_millis(500)).await
    }
}

impl MessageCaptureChannels {
    /// Create a new message capture with default buffer size (10)
    pub fn new() -> Self {
        Self::with_buffer_size(10)
    }

    /// Create a new message capture with the specified channel buffer size
    pub fn with_buffer_size(buffer_size: usize) -> Self {
        let (tx_out, rx_out) = mpsc::channel(buffer_size);
        let (tx_in, rx_in) = mpsc::channel(buffer_size);

        Self {
            capture: MessageCapture {
                rx: rx_out,
                _tx: tx_in.clone(),
            },
            tx_out,
            rx_in,
        }
    }
}

impl Default for MessageCaptureChannels {
    fn default() -> Self {
        Self::new()
    }
}
