//! In-memory transport for deterministic tests.
//!
//! Provides bidirectional mock connections, error injection, and simple
//! client/server helpers without network I/O.

use crate::error::TransportError;
use crate::transport::{EstablishedConnection, TransportClient, TransportServer};
use crate::types::{PeerTLSIdentity, TransportFrame};
use async_trait::async_trait;
use std::io;
use tokio::sync::{Mutex, mpsc};

/// In-memory transport connection used for tests.
///
/// Sends and receives framed `TransportFrame` via channels.
pub struct MockConnection {
    tx: mpsc::Sender<TransportFrame>,
    rx: mpsc::Receiver<TransportFrame>,
    /// Connection identifier (for debugging/assertions).
    peer_id: String,
    inject_error: Option<TransportError>,
    /// Optional TLS-based peer identity.
    peer_identity: Option<PeerTLSIdentity>,
}

impl MockConnection {
    /// Return the peer ID string.
    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }

    /// Inject a single-use error returned by the next `send`/`recv`.
    pub fn inject_error(&mut self, error: TransportError) {
        self.inject_error = Some(error);
    }

    /// Set the optional TLS peer identity used by the connection.
    pub fn with_peer_identity(mut self, identity: Option<PeerTLSIdentity>) -> Self {
        self.peer_identity = identity;
        self
    }

    /// Send method for testing (not part of trait).
    pub async fn send(&mut self, frame: TransportFrame) -> Result<(), TransportError> {
        if let Some(error) = self.inject_error.take() {
            return Err(error);
        }

        self.tx
            .send(frame)
            .await
            .map_err(|e| TransportError::ConnectionClosed(io::Error::other(e.to_string())))
    }

    /// Recv method for testing (not part of trait).
    pub async fn recv(&mut self) -> Result<TransportFrame, TransportError> {
        if let Some(error) = self.inject_error.take() {
            return Err(error);
        }

        match self.rx.recv().await {
            Some(frame) => Ok(frame),
            None => Err(TransportError::ConnectionClosed(io::Error::other(
                "connection closed",
            ))),
        }
    }

    /// Convert this MockConnection into an EstablishedConnection by starting the I/O loop.
    pub fn into_established(self) -> EstablishedConnection {
        let MockConnection {
            tx,
            rx,
            peer_id,
            inject_error,
            peer_identity,
        } = self;
        let (result_tx, result_rx) = mpsc::channel(32);

        // Spawn a task to convert TransportFrame to Result<TransportFrame, TransportError>
        let handle = tokio::spawn(async move {
            let mut rx = rx;
            if let Some(error) = inject_error {
                let _ = result_tx.send(Err(error)).await;
                return;
            }
            while let Some(frame) = rx.recv().await {
                if result_tx.send(Ok(frame)).await.is_err() {
                    break;
                }
            }
        });

        let watcher = Box::pin(async move {
            let _ = handle.await;
        });

        EstablishedConnection {
            tx,
            rx: result_rx,
            watcher,
            peer_addr: format!("mock:{}", peer_id),
            peer_identity,
        }
    }
}

#[async_trait]
impl TransportServer for MockConnection {
    async fn accept(&mut self) -> Result<EstablishedConnection, TransportError> {
        unimplemented!("MockConnection does not implement TransportServer")
    }
}

/// Create a connected pair of `MockConnection` instances.
pub fn create_mock_pair(base_id: &str) -> (MockConnection, MockConnection) {
    let (tx_a, rx_a) = mpsc::channel(32);
    let (tx_b, rx_b) = mpsc::channel(32);

    let conn_a = MockConnection {
        tx: tx_b,
        rx: rx_a,
        peer_id: format!("{}_a", base_id),
        inject_error: None,
        peer_identity: None,
    };

    let conn_b = MockConnection {
        tx: tx_a,
        rx: rx_b,
        peer_id: format!("{}_b", base_id),
        inject_error: None,
        peer_identity: None,
    };

    (conn_a, conn_b)
}

/// Server that returns provided connections on `accept()`.
pub struct MockServer {
    /// Queue of connections to return from accept().
    connections: Vec<EstablishedConnection>,
}

impl MockServer {
    /// Construct a new `MockServer` returning the supplied connections.
    pub fn new(connections: Vec<EstablishedConnection>) -> Self {
        Self { connections }
    }
}

#[async_trait]
impl TransportServer for MockServer {
    async fn accept(&mut self) -> Result<EstablishedConnection, TransportError> {
        match self.connections.pop() {
            Some(conn) => Ok(conn),
            None => Err(TransportError::ConnectionClosed(std::io::Error::other(
                "No more connections to accept",
            ))),
        }
    }
}

type ConnectionResult = Result<EstablishedConnection, TransportError>;

/// Client that returns a preconfigured connection or error on `connect()`.
pub struct MockClient {
    /// Connection to return from connect() (or error to return).
    connection: Mutex<Option<ConnectionResult>>,
}

impl MockClient {
    /// Create a client that yields `connection` once from `connect()`.
    pub fn with_connection(connection: EstablishedConnection) -> Self {
        Self {
            connection: Mutex::new(Some(Ok(connection))),
        }
    }

    /// Create a client that yields `error` once from `connect()`.
    pub fn with_error(error: TransportError) -> Self {
        Self {
            connection: Mutex::new(Some(Err(error))),
        }
    }
}

#[async_trait]
impl TransportClient for MockClient {
    async fn connect(&self) -> Result<EstablishedConnection, TransportError> {
        // Return an error instead of panicking when there's no configured
        // connection to return. Tests expect an Err rather than a panic.
        match self.connection.lock().await.take() {
            Some(conn_res) => conn_res,
            None => Err(TransportError::ConnectionClosed(std::io::Error::other(
                "Connection already consumed",
            ))),
        }
    }
}
