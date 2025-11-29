//! In-memory transport for deterministic tests.
//!
//! Provides bidirectional mock connections, error injection, and simple
//! client/server helpers without network I/O.

use crate::error::TransportError;
use crate::transport::{EstablishedConnection, TransportClient, TransportServer};
use crate::types::{PeerTLSIdentity, TransportFrame};
use async_trait::async_trait;
use std::{collections::VecDeque, io};
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

/// A handle that allows tests to sever a connection (simulate cable cut).
#[derive(Clone)]
pub struct KillSwitch {
    trigger: tokio::sync::watch::Sender<bool>,
}

impl KillSwitch {
    /// Sever the connection by notifying watchers.
    pub fn sever(&self) {
        // Ignore error: there may be no receivers left
        let _ = self.trigger.send(true);
    }
}

/// Create a connected pair of `EstablishedConnection` instances plus a `KillSwitch`.
///
/// The returned `EstablishedConnection`s behave like the ones from `into_established()`,
/// but the `KillSwitch` can be used to forcibly close the transport tasks and
/// make the watcher future complete (simulating a cable cut).
pub fn create_controlled_pair(
    base_id: &str,
) -> (EstablishedConnection, EstablishedConnection, KillSwitch) {
    use crate::error::TransportError;
    use std::io;

    let (tx_a, rx_a) = mpsc::channel(32);
    let (tx_b, rx_b) = mpsc::channel(32);

    // Channels to communicate framed Results to consumers
    let (result_tx_a, result_rx_a) = mpsc::channel(32);
    let (result_tx_b, result_rx_b) = mpsc::channel(32);

    // Watch channel for kill signal
    let (kill_tx, mut kill_rx) = tokio::sync::watch::channel(false);

    // Spawn task to forward rx_a -> result_tx_a, with kill support.
    // Added debug logs to trace frame-level forwarding and kill events.
    let handle_a = tokio::spawn(async move {
        let mut rx: mpsc::Receiver<TransportFrame> = rx_a;
        let result_tx: mpsc::Sender<Result<TransportFrame, TransportError>> = result_tx_a;
        loop {
            tokio::select! {
                biased;
                _ = kill_rx.changed() => {
                    if *kill_rx.borrow() {
                        let _ = result_tx.send(Err(TransportError::ConnectionClosed(io::Error::other("killed")))).await;
                        tracing::debug!("create_controlled_pair: side A killed, sent ConnectionClosed to consumer");
                        return;
                    }
                }
                maybe_frame = rx.recv() => {
                    match maybe_frame {
                        Some(frame) => {
                            // Log a compact hex prefix of the frame bytes to help trace
                            let bytes = frame.get_bytes();
                            let prefix: String = bytes
                                .iter()
                                .take(32)
                                .map(|b| format!("{:02x}", b))
                                .collect::<Vec<_>>()
                                .join("");
                            tracing::trace!("create_controlled_pair: side A forwarding frame bytes_len={} prefix={}...", bytes.len(), prefix);
                            if result_tx.send(Ok(frame)).await.is_err() {
                                tracing::debug!("create_controlled_pair: side A result_tx closed while forwarding");
                                break;
                            }
                        }
                        None => {
                            // Sender dropped -> connection closed
                            tracing::debug!("create_controlled_pair: side A rx closed (peer dropped)");
                            break;
                        }
                    }
                }
            }
        }
    });

    // Need a separate receiver for the other side of the watch
    let mut kill_rx_b = kill_tx.subscribe();

    let handle_b = tokio::spawn(async move {
        let mut rx: mpsc::Receiver<TransportFrame> = rx_b;
        let result_tx: mpsc::Sender<Result<TransportFrame, TransportError>> = result_tx_b;
        loop {
            tokio::select! {
                biased;
                _ = kill_rx_b.changed() => {
                    if *kill_rx_b.borrow() {
                        let _ = result_tx.send(Err(TransportError::ConnectionClosed(io::Error::other("killed")))).await;
                        tracing::debug!("create_controlled_pair: side B killed, sent ConnectionClosed to consumer");
                        return;
                    }
                }
                maybe_frame = rx.recv() => {
                    match maybe_frame {
                        Some(frame) => {
                            // Log a compact hex prefix of the frame bytes to help trace
                            let bytes = frame.get_bytes();
                            let prefix: String = bytes
                                .iter()
                                .take(32)
                                .map(|b| format!("{:02x}", b))
                                .collect::<Vec<_>>()
                                .join("");
                            tracing::trace!("create_controlled_pair: side B forwarding frame bytes_len={} prefix={}...", bytes.len(), prefix);
                            if result_tx.send(Ok(frame)).await.is_err() {
                                tracing::debug!("create_controlled_pair: side B result_tx closed while forwarding");
                                break;
                            }
                        }
                        None => {
                            tracing::debug!("create_controlled_pair: side B rx closed (peer dropped)");
                            break;
                        }
                    }
                }
            }
        }
    });

    let watcher_a = Box::pin(async move {
        let _ = handle_a.await;
    });

    let watcher_b = Box::pin(async move {
        let _ = handle_b.await;
    });

    let conn_a = EstablishedConnection {
        tx: tx_b,
        rx: result_rx_a,
        watcher: watcher_a,
        peer_addr: format!("mock:{}_a", base_id),
        peer_identity: None,
    };

    let conn_b = EstablishedConnection {
        tx: tx_a,
        rx: result_rx_b,
        watcher: watcher_b,
        peer_addr: format!("mock:{}_b", base_id),
        peer_identity: None,
    };

    (conn_a, conn_b, KillSwitch { trigger: kill_tx })
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

/// Client that returns a sequence of configured connections/errors on `connect()`.
///
/// Internally holds a `VecDeque<ConnectionResult>` so tests can push additional
/// connections (or errors) to simulate reconnects and restores.
pub struct MockClient {
    queue: Mutex<VecDeque<ConnectionResult>>,
}

impl Default for MockClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockClient {
    /// Create an empty client; tests can push results later.
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
        }
    }

    /// Create a client that yields `connection` first from `connect()`.
    pub fn with_connection(connection: EstablishedConnection) -> Self {
        let mut dq = VecDeque::new();
        dq.push_back(Ok(connection));
        Self {
            queue: Mutex::new(dq),
        }
    }

    /// Create a client that yields `error` first from `connect()`.
    pub fn with_error(error: TransportError) -> Self {
        let mut dq = VecDeque::new();
        dq.push_back(Err(error));
        Self {
            queue: Mutex::new(dq),
        }
    }

    /// Push an `EstablishedConnection` to be returned by the next `connect()`.
    pub async fn push_connection(&self, connection: EstablishedConnection) {
        self.queue.lock().await.push_back(Ok(connection));
    }

    /// Push an `TransportError` to be returned by the next `connect()`.
    pub async fn push_error(&self, error: TransportError) {
        self.queue.lock().await.push_back(Err(error));
    }
}

#[async_trait]
impl TransportClient for MockClient {
    async fn connect(&self) -> Result<EstablishedConnection, TransportError> {
        // Pop the next configured result from the queue. If none is available,
        // return an error so tests can handle retries/backoff deterministically.
        match self.queue.lock().await.pop_front() {
            Some(conn_res) => conn_res,
            None => Err(TransportError::ConnectionClosed(std::io::Error::other(
                "No connection available",
            ))),
        }
    }
}

#[async_trait]
impl TransportClient for std::sync::Arc<MockClient> {
    async fn connect(&self) -> Result<EstablishedConnection, TransportError> {
        match self.queue.lock().await.pop_front() {
            Some(conn_res) => conn_res,
            None => Err(TransportError::ConnectionClosed(std::io::Error::other(
                "No connection available",
            ))),
        }
    }
}
