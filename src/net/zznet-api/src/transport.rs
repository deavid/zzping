//! Transport layer abstractions (framing and I/O).
//!
//! These traits define the low-level I/O and framing contract used by
//! higher layers. Protocol logic and serialization belong to upper layers.

use crate::error::TransportError;
use crate::types::TransportFrame;
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Low-level bidirectional transport connection.
///
/// Implementations provide framed I/O for higher-level protocols.
///
/// Identity: TLS-backed connections expose a verified `peer_tls_identity`;
/// raw transports may not provide cryptographic identity — use higher-layer
/// validation for application-level trust decisions.
#[async_trait]
pub trait TransportConnection: Send {
    /// Starts the transport's active I/O tasks and returns channels for communication.
    ///
    /// This method consumes the connection and spawns background tasks for reading
    /// and writing. The returned sender is used to send outbound frames, and the
    /// receiver yields inbound frames or errors.
    ///
    /// The lifecycle is: Configure -> Inspect Identity -> Start IO.
    fn start(
        self: Box<Self>,
    ) -> (
        mpsc::Sender<TransportFrame>,
        mpsc::Receiver<Result<TransportFrame, TransportError>>,
    );

    /// Peer address for logging/metrics, if available.
    fn peer_addr(&self) -> Option<String>;

    /// Optional TLS-verified peer identity.
    fn peer_tls_identity(&self) -> Option<crate::types::PeerTLSIdentity>;
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
    async fn accept(&mut self) -> Result<Box<dyn TransportConnection>, TransportError>;
}

/// Outgoing connection factory used to create new `TransportConnection`s.
///
/// `connect()` establishes a new connection; clients are configuration
/// wrappers and may be called repeatedly to create multiple connections.
///
/// Reconnection is the responsibility of callers/upper layers.
#[async_trait]
pub trait TransportClient: Send {
    /// Establish a new connection to the configured server.
    async fn connect(&self) -> Result<Box<dyn TransportConnection>, TransportError>;
}
