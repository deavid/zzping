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

// create_peer_with_message_capture() removed in Phase 8 (used SessionManager)
// Tests should create peers and channels directly using PeerSession::new_connected()

// SessionManager-based test utilities removed in Phase 8
// These functions were deprecated and unused. Tests should use PeerManagerActor directly.
// Removed functions:
// - connect_managers_in_memory()
// - create_and_add_peer()
// - create_peer_with_message_capture()

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
///
/// # Example
/// ```rust,no_run,ignore
/// use zzping_test_utils::MessageCaptureChannels;
/// use zznet_api::types::RoomId;
///
/// // Old way (boilerplate):
/// // let (tx_out, mut rx_out) = mpsc::channel(10);
/// // let (_tx_in, rx_in) = mpsc::channel(10);
/// // let pkt = timeout(Duration::from_millis(500), rx_out.recv()).await;
///
/// // New way (simple):
/// async fn example() {
///     let channels: MessageCaptureChannels = MessageCaptureChannels::new();
///     let mut capture = channels.capture;
///     let pkt = capture.recv_default_timeout().await;
/// }
/// ```
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

// create_peer_with_message_capture() removed in Phase 8 (used SessionManager)
// Tests should create peers and channels directly using PeerSession::new_connected()
