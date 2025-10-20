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
use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::types::{RoomId, SessionError};

/// A minimal RoomHandle implementation used in tests to provide a room id
/// for PeerSession so joined_rooms can be negotiated. This does not process
/// inbound messages — it's only to satisfy PeerSession invariants.
pub struct DummyRoomHandle {
    id: RoomId,
}

impl DummyRoomHandle {
    /// Create a new `DummyRoomHandle` with the provided `RoomId`.
    ///
    /// This handle does not process messages; it only provides a stable
    /// `room_id()` implementation for use in tests where a `RoomHandle` is required.
    pub fn new(id: RoomId) -> Self {
        Self { id }
    }
}

/// Connect two SessionManager instances in-memory by wiring their peer channels
/// together using tokio mpsc channels. This creates bidirectional channels so
/// that manager_a can send to peer_b and manager_b can send to peer_a.
pub async fn connect_managers_in_memory<TMsg, TRole>(
    manager_a: &mut zznet_session::session_manager::SessionManager<TMsg, TRole>,
    peer_id_a: &zznet_session::types::PeerId,
    manager_b: &mut zznet_session::session_manager::SessionManager<TMsg, TRole>,
    peer_id_b: &zznet_session::types::PeerId,
) -> Result<(), zznet_session::types::SessionError>
where
    TMsg: RoomMessageTrait,
    TRole: zznet_auth::ApplicationRole,
{
    use tokio::sync::mpsc;

    // Channel: A -> B
    let (tx_a_to_b, rx_a_to_b) = mpsc::channel::<(RoomId, TMsg)>(16);
    // Channel: B -> A
    let (tx_b_to_a, rx_b_to_a) = mpsc::channel::<(RoomId, TMsg)>(16);

    // Manager A connects to peer B with outbound tx A->B and inbound rx B->A
    manager_a
        .connect_peer(peer_id_b.clone(), tx_a_to_b, rx_b_to_a)
        .await?;

    // Manager B connects to peer A with outbound tx B->A and inbound rx A->B
    manager_b
        .connect_peer(peer_id_a.clone(), tx_b_to_a, rx_a_to_b)
        .await?;

    Ok(())
}

/// Convenience helper to create a fully configured peer with rooms and permissions.
/// This reduces boilerplate in tests by handling the common pattern of:
/// 1. Creating a peer
/// 2. Adding rooms
/// 3. Setting role/permissions
/// 4. Adding to manager
/// 5. Publishing rooms
pub async fn create_and_add_peer<TMsg, TRole>(
    manager: &mut zznet_session::session_manager::SessionManager<TMsg, TRole>,
    peer_id: &zznet_session::types::PeerId,
    rooms: Vec<RoomId>,
    role: Option<TRole>,
) -> Result<(), zznet_session::types::SessionError>
where
    TMsg: RoomMessageTrait,
    TRole: zznet_auth::ApplicationRole,
{
    // Create peer
    let mut peer = zznet_session::peer_session::PeerSession::<TMsg, TRole>::new(peer_id.clone());

    // Add rooms
    for room_id in &rooms {
        peer.add_room(
            room_id.clone(),
            Box::new(DummyRoomHandle::new(room_id.clone())),
        )
        .await?;
    }

    // Set role if provided
    if let Some(role) = role {
        peer.set_role(Some(role));
    }

    // Add peer to manager
    manager.add_peer(peer_id.clone(), peer)?;

    // Publish rooms
    manager.handle_publish_rooms(peer_id, rooms)?;

    Ok(())
}

/// Return type for MessageCapture creation that bundles the capture handle with connection channels
pub struct MessageCaptureChannels<TMsg> {
    /// The capture handle used to receive messages from the connected peer.
    pub capture: MessageCapture<TMsg>,
    /// Sender side for outbound messages into the peer under test.
    pub tx_out: mpsc::Sender<(RoomId, TMsg)>,
    /// Receiver side for inbound messages that will be delivered to the peer.
    pub rx_in: mpsc::Receiver<(RoomId, TMsg)>,
}

/// Message capture helper that sets up channels and provides a way to wait for messages.
/// This reduces boilerplate for the common pattern of:
/// 1. Creating channels
/// 2. Connecting peer
/// 3. Waiting for messages with timeout
///
/// # Example
/// ```rust,no_run
/// use zzping_test_utils::MessageCaptureChannels;
/// use zznet_session::types::RoomId;
///
/// // Old way (boilerplate):
/// // let (tx_out, mut rx_out) = mpsc::channel(10);
/// // let (_tx_in, rx_in) = mpsc::channel(10);
/// // let pkt = timeout(Duration::from_millis(500), rx_out.recv()).await;
///
/// // New way (simple):
/// async fn example() {
///     let channels: MessageCaptureChannels<String> = MessageCaptureChannels::new();
///     let mut capture = channels.capture;
///     let pkt = capture.recv_default_timeout().await;
/// }
/// ```
pub struct MessageCapture<TMsg> {
    /// Receiver for captured messages. Tests can `.recv()` on this to observe
    /// messages published by the peer under test.
    pub rx: mpsc::Receiver<(RoomId, TMsg)>,
    // Internal sender kept alive so the channel remains open while the
    // capture exists.
    _tx: mpsc::Sender<(RoomId, TMsg)>, // Keep alive
}

impl<TMsg> MessageCapture<TMsg>
where
    TMsg: Send + 'static,
{
    /// Wait for the next message with a timeout
    pub async fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<(RoomId, TMsg)>, tokio::time::error::Elapsed> {
        tokio::time::timeout(timeout, self.rx.recv()).await
    }

    /// Wait for the next message with default timeout (500ms)
    pub async fn recv_default_timeout(
        &mut self,
    ) -> Result<Option<(RoomId, TMsg)>, tokio::time::error::Elapsed> {
        self.recv_timeout(Duration::from_millis(500)).await
    }
}

impl<TMsg> MessageCaptureChannels<TMsg> {
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

impl<TMsg> Default for MessageCaptureChannels<TMsg> {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience helper that creates a peer, connects it to capture messages, and returns the capture handle.
/// This combines create_and_add_peer + MessageCapture setup for the most common test pattern.
pub async fn create_peer_with_message_capture<TMsg, TRole>(
    manager: &mut zznet_session::session_manager::SessionManager<TMsg, TRole>,
    peer_id: &zznet_session::types::PeerId,
    rooms: Vec<RoomId>,
    role: Option<TRole>,
) -> Result<MessageCapture<TMsg>, zznet_session::types::SessionError>
where
    TMsg: RoomMessageTrait,
    TRole: zznet_auth::ApplicationRole,
{
    // Create and add the peer
    create_and_add_peer(manager, peer_id, rooms, role).await?;

    // Set up message capture
    let channels = MessageCaptureChannels::new();

    // Connect the peer
    manager
        .connect_peer(peer_id.clone(), channels.tx_out, channels.rx_in)
        .await?;

    Ok(channels.capture)
}

impl<M: Send + 'static + RoomMessageTrait> zznet_session::peer_session::RoomHandle<M>
    for DummyRoomHandle
{
    fn room_id(&self) -> &RoomId {
        &self.id
    }

    fn send_message(&mut self, _msg: M) -> Result<(), SessionError> {
        // No-op for tests
        Ok(())
    }

    fn spawn_forwarder(&mut self, _tx: mpsc::Sender<(RoomId, M)>) -> Result<(), SessionError> {
        // No-op for tests
        Ok(())
    }
}
