//! Transport layer abstractions (framing and I/O).
//!
//! These traits define the low-level I/O and framing contract used by
//! higher layers. Protocol logic and serialization belong to upper layers.

use crate::error::TransportError;
use crate::types::{PeerTLSIdentity, TransportFrame};
use async_trait::async_trait;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc;

/// A future that resolves when the transport connection is closed.
///
/// The transport layer provider is responsible for ensuring this future
/// completes when the underlying I/O tasks terminate (either due to error,
/// peer disconnect, or the local actor dropping the channels).
pub type TransportWatcher = Pin<Box<dyn Future<Output = ()> + Send>>;

/// An established transport connection with active I/O tasks.
///
/// This represents a bidirectional connection that is ready to exchange frames.
/// The I/O tasks are already running in the background.
pub struct EstablishedConnection {
    /// Sender for outbound frames to the peer.
    pub tx: mpsc::Sender<TransportFrame>,
    /// Receiver for inbound frames from the peer.
    pub rx: mpsc::Receiver<Result<TransportFrame, TransportError>>,
    /// A future that resolves when the connection is closed.
    pub watcher: TransportWatcher,
    /// Peer address for logging/metrics, if available.
    pub peer_addr: String,
    /// Optional TLS-verified peer identity.
    pub peer_identity: Option<PeerTLSIdentity>,
}

/// Server that accepts incoming transport connections.
///
/// Usage: `accept()` yields established connections; dropping stops listening.
///
/// Error handling: transient failures are implementation-specific; `accept`
/// only returns fatal errors that prevent further accepts.
#[async_trait]
pub trait TransportServer: Send {
    /// Accept a new established connection, or return a fatal error.
    async fn accept(&mut self) -> Result<EstablishedConnection, TransportError>;
}

/// Outgoing connection factory used to create new connections.
///
/// `connect()` establishes a new connection; clients are configuration
/// wrappers and may be called repeatedly to create multiple connections.
///
/// Reconnection is the responsibility of callers/upper layers.
#[async_trait]
pub trait TransportClient: Send {
    /// Establish a new connection to the configured server.
    async fn connect(&self) -> Result<EstablishedConnection, TransportError>;
}
