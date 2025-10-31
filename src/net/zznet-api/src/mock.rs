//! Mock transport implementation for testing.
//!
//! This module provides a production-quality mock transport that enables
//! testing the entire network stack without any real network I/O.
//!
//! ## Design Philosophy
//!
//! The mock transport is NOT a toy or placeholder - it's a first-class
//! implementation that must be:
//! - Feature-complete (all transport features work)
//! - Fast (microsecond latency, no actual I/O)
//! - Deterministic (no timing-based flakiness)
//! - Flexible (error injection, controllable behavior)
//!
//! ## Architecture Validation
//!
//! If the network stack works perfectly with mock transport but fails with
//! real transport, the abstraction has leaked. Mock transport serves as
//! continuous validation that the layers are properly separated.

use crate::error::TransportError;
use crate::transport::{TransportClient, TransportConnection, TransportServer};
use crate::types::PeerIdentity;
use async_trait::async_trait;
use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

/// Maximum frame size for mock transport (16 MiB).
///
/// This matches the recommended limit for real transport implementations.
const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// A mock transport connection using in-memory channels.
///
/// Each connection has a send and receive channel. Messages sent on one
/// connection are received on its peer connection.
///
/// ## Lifecycle
///
/// - Created via `create_mock_pair()` which returns two connected instances
/// - `recv()` returns `None` when the peer drops their connection
/// - Dropping closes the connection gracefully
pub struct MockConnection {
    /// Channel to send frames to the peer.
    tx: mpsc::Sender<Bytes>,
    /// Channel to receive frames from the peer.
    rx: Mutex<mpsc::Receiver<Bytes>>,
    /// Identifier for this connection (for logging/debugging).
    peer_id: String,
    /// Optional error to inject on next operation.
    inject_error: Arc<Mutex<Option<TransportError>>>,
    /// The peer identity for this connection (None for plain TCP, Some for TLS).
    peer_identity: Option<PeerIdentity>,
}

impl MockConnection {
    /// Returns the peer ID for this connection.
    ///
    /// Useful for debugging and test assertions.
    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }

    /// Injects an error that will be returned on the next send or recv operation.
    ///
    /// This allows testing error handling paths without complex mock setup.
    /// The error is consumed after being returned once.
    ///
    /// # Example
    ///
    /// ```
    /// # use zznet_api::mock::create_mock_pair;
    /// # use zznet_api::error::TransportError;
    /// # use zznet_api::transport::TransportConnection;
    /// # use bytes::Bytes;
    /// # let rt = tokio::runtime::Runtime::new().unwrap();
    /// # rt.block_on(async {
    /// let (mut conn_a, mut conn_b) = create_mock_pair("test");
    /// conn_a.inject_error(TransportError::Timeout).await;
    /// let result = conn_a.recv().await;
    /// assert!(matches!(result, Err(TransportError::Timeout)));
    /// # });
    /// ```
    pub async fn inject_error(&self, error: TransportError) {
        *self.inject_error.lock().await = Some(error);
    }

    /// Sets the peer identity for this connection.
    ///
    /// This allows tests to configure specific identities for ACL testing.
    /// By default, mock connections may have None (plain TCP) or Some identity (TLS).
    pub fn with_peer_identity(mut self, identity: Option<PeerIdentity>) -> Self {
        self.peer_identity = identity;
        self
    }
}

#[async_trait]
impl TransportConnection for MockConnection {
    async fn send(&mut self, frame: Bytes) -> Result<(), TransportError> {
        // Check for injected error first
        if let Some(error) = self.inject_error.lock().await.take() {
            return Err(error);
        }

        // Enforce frame size limit
        if frame.len() > MAX_FRAME_SIZE {
            return Err(TransportError::FrameTooLarge {
                size: frame.len(),
                limit: MAX_FRAME_SIZE,
            });
        }

        // Send to peer (channel closed = connection closed)
        self.tx
            .send(frame)
            .await
            .map_err(|_| TransportError::ConnectionClosed)
    }

    async fn recv(&mut self) -> Result<Option<Bytes>, TransportError> {
        // Check for injected error first
        if let Some(error) = self.inject_error.lock().await.take() {
            return Err(error);
        }

        // Receive from peer (None = graceful close)
        let mut rx = self.rx.lock().await;
        Ok(rx.recv().await)
    }

    fn peer_addr(&self) -> Option<String> {
        Some(format!("mock:{}", self.peer_id))
    }

    fn peer_identity(&self) -> Option<PeerIdentity> {
        self.peer_identity.clone()
    }
}

/// Creates a pair of connected mock connections.
///
/// The two connections are wired together: messages sent on connection A
/// are received on connection B, and vice versa.
///
/// This is the primary way to create mock connections for testing.
///
/// # Example
///
/// ```
/// # use zznet_api::mock::create_mock_pair;
/// # use zznet_api::transport::TransportConnection;
/// # use bytes::Bytes;
/// # let rt = tokio::runtime::Runtime::new().unwrap();
/// # rt.block_on(async {
/// let (mut conn_a, mut conn_b) = create_mock_pair("test");
///
/// // Send from A to B
/// conn_a.send(Bytes::from("hello")).await.unwrap();
/// let msg = conn_b.recv().await.unwrap();
/// assert_eq!(msg, Some(Bytes::from("hello")));
/// # });
/// ```
pub fn create_mock_pair(base_id: &str) -> (MockConnection, MockConnection) {
    let (tx_a, rx_a) = mpsc::channel(32);
    let (tx_b, rx_b) = mpsc::channel(32);

    let conn_a = MockConnection {
        tx: tx_b,
        rx: Mutex::new(rx_a),
        peer_id: format!("{}_a", base_id),
        inject_error: Arc::new(Mutex::new(None)),
        peer_identity: None, // Plain TCP by default
    };

    let conn_b = MockConnection {
        tx: tx_a,
        rx: Mutex::new(rx_b),
        peer_id: format!("{}_b", base_id),
        inject_error: Arc::new(Mutex::new(None)),
        peer_identity: None, // Plain TCP by default
    };

    (conn_a, conn_b)
}

/// A mock transport server that yields pre-configured connections.
///
/// Instead of actually listening on a network socket, this server returns
/// connections from a queue that you provide. This allows complete control
/// over what connections are "accepted" during tests.
pub struct MockServer {
    /// Queue of connections to return from accept().
    connections: Arc<Mutex<Vec<Box<dyn TransportConnection>>>>,
}

impl MockServer {
    /// Creates a new mock server with the given connections.
    ///
    /// Connections will be returned in the order provided.
    pub fn new(connections: Vec<Box<dyn TransportConnection>>) -> Self {
        Self {
            connections: Arc::new(Mutex::new(connections)),
        }
    }
}

#[async_trait]
impl TransportServer for MockServer {
    async fn accept(&mut self) -> Result<Box<dyn TransportConnection>, TransportError> {
        let mut conns = self.connections.lock().await;
        conns.pop().ok_or(TransportError::InvalidState(
            "No more connections".to_string(),
        ))
    }
}

type ConnectionResult = Result<Box<dyn TransportConnection>, TransportError>;

/// A mock transport client that returns pre-configured connections.
///
/// Similar to `MockServer`, but for outgoing connections. You configure
/// what connection should be returned when `connect()` is called.
pub struct MockClient {
    /// Connection to return from connect() (or error to return).
    connection: Arc<Mutex<Option<ConnectionResult>>>,
}

impl MockClient {
    /// Creates a new mock client that returns the given connection.
    pub fn with_connection(connection: Box<dyn TransportConnection>) -> Self {
        Self {
            connection: Arc::new(Mutex::new(Some(Ok(connection)))),
        }
    }

    /// Creates a new mock client that returns an error.
    pub fn with_error(error: TransportError) -> Self {
        Self {
            connection: Arc::new(Mutex::new(Some(Err(error)))),
        }
    }
}

#[async_trait]
impl TransportClient for MockClient {
    async fn connect(&self) -> Result<Box<dyn TransportConnection>, TransportError> {
        self.connection
            .lock()
            .await
            .take()
            .ok_or(TransportError::InvalidState(
                "Connection already consumed".to_string(),
            ))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_pair_bidirectional_communication() {
        let (mut conn_a, mut conn_b) = create_mock_pair("test");

        // A sends to B
        conn_a.send(Bytes::from("hello")).await.unwrap();
        let msg = conn_b.recv().await.unwrap();
        assert_eq!(msg, Some(Bytes::from("hello")));

        // B sends to A
        conn_b.send(Bytes::from("world")).await.unwrap();
        let msg = conn_a.recv().await.unwrap();
        assert_eq!(msg, Some(Bytes::from("world")));
    }

    #[tokio::test]
    async fn test_mock_pair_graceful_close() {
        let (conn_a, mut conn_b) = create_mock_pair("test");

        // Drop A, B should see graceful close
        drop(conn_a);
        let msg = conn_b.recv().await.unwrap();
        assert_eq!(msg, None);
    }

    #[tokio::test]
    async fn test_mock_pair_send_after_close() {
        let (mut conn_a, conn_b) = create_mock_pair("test");

        // Drop B
        drop(conn_b);

        // A should get error when trying to send
        let result = conn_a.send(Bytes::from("test")).await;
        assert!(matches!(result, Err(TransportError::ConnectionClosed)));
    }

    #[tokio::test]
    async fn test_mock_connection_frame_size_limit() {
        let (mut conn_a, mut _conn_b) = create_mock_pair("test");

        // Try to send frame larger than limit
        let large_frame = Bytes::from(vec![0u8; MAX_FRAME_SIZE + 1]);
        let result = conn_a.send(large_frame).await;

        assert!(matches!(result, Err(TransportError::FrameTooLarge { .. })));
    }

    #[tokio::test]
    async fn test_mock_connection_error_injection() {
        let (mut conn_a, _conn_b) = create_mock_pair("test");

        // Inject timeout error
        conn_a.inject_error(TransportError::Timeout).await;

        // Next operation should return the injected error
        let result = conn_a.send(Bytes::from("test")).await;
        assert!(matches!(result, Err(TransportError::Timeout)));

        // Subsequent operations should work normally
        let result = conn_a.send(Bytes::from("test")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_connection_peer_addr() {
        let (conn_a, _conn_b) = create_mock_pair("test");

        assert_eq!(conn_a.peer_addr(), Some("mock:test_a".to_string()));
    }

    #[tokio::test]
    async fn test_mock_server_accepts_connections() {
        let (conn_a, _conn_b) = create_mock_pair("test");
        let mut server = MockServer::new(vec![Box::new(conn_a)]);

        let conn = server.accept().await.unwrap();
        assert!(conn.peer_addr().is_some());

        // Second accept should fail (no more connections)
        let result = server.accept().await;
        assert!(matches!(result, Err(TransportError::InvalidState(_))));
    }

    #[tokio::test]
    async fn test_mock_client_connects() {
        let (conn_a, _conn_b) = create_mock_pair("test");
        let client = MockClient::with_connection(Box::new(conn_a));

        let conn = client.connect().await.unwrap();
        assert!(conn.peer_addr().is_some());

        // Second connect should fail (connection consumed)
        let result = client.connect().await;
        assert!(matches!(result, Err(TransportError::InvalidState(_))));
    }

    #[tokio::test]
    async fn test_mock_client_with_error() {
        let client = MockClient::with_error(TransportError::Timeout);

        let result = client.connect().await;
        assert!(matches!(result, Err(TransportError::Timeout)));
    }

    #[tokio::test]
    async fn test_mock_pair_multiple_messages() {
        let (mut conn_a, mut conn_b) = create_mock_pair("test");

        // Send multiple messages in sequence
        for i in 0..10 {
            let msg = format!("message_{}", i);
            conn_a.send(Bytes::from(msg.clone())).await.unwrap();
            let received = conn_b.recv().await.unwrap().unwrap();
            assert_eq!(received, Bytes::from(msg));
        }
    }

    #[tokio::test]
    async fn test_mock_pair_zero_length_frame() {
        let (mut conn_a, mut conn_b) = create_mock_pair("test");

        // Zero-length frames are valid (can be used for heartbeats)
        conn_a.send(Bytes::new()).await.unwrap();
        let msg = conn_b.recv().await.unwrap();
        assert_eq!(msg, Some(Bytes::new()));
    }
}
