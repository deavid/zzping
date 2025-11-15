//! Transport layer abstractions (framing and I/O).
//!
//! These traits define the low-level I/O and framing contract used by
//! higher layers. Protocol logic and serialization belong to upper layers.

use crate::error::TransportError;
use async_trait::async_trait;
use bytes::Bytes;

/// Low-level bidirectional transport connection.
///
/// Implementations provide framed I/O for higher-level protocols.
///
/// Framing: 4-byte big-endian length prefix, payload follows; zero-length frames
/// are valid; implementations should enforce a maximum frame size.
///
/// Identity: TLS-backed connections expose a verified `peer_tls_identity`;
/// raw transports may not provide cryptographic identity — use higher-layer
/// validation for application-level trust decisions.
#[async_trait]
pub trait TransportConnection: Send {
    /// Send a framed `Bytes` over the connection.
    async fn send(&mut self, frame: Bytes) -> Result<(), TransportError>;

    /// Receive a framed `Bytes`.
    async fn recv(&mut self) -> Result<Bytes, TransportError>;

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

#[cfg(test)]
mod tests {
    use super::*;

    // These tests verify that the traits are object-safe and can be used
    // as trait objects (Box<dyn Trait>).

    #[test]
    fn test_transport_connection_is_object_safe() {
        // This test compiles if TransportConnection is object-safe
        fn _takes_boxed(_conn: Box<dyn TransportConnection>) {}
    }

    #[test]
    fn test_transport_server_is_object_safe() {
        // This test compiles if TransportServer is object-safe
        fn _takes_boxed(_server: Box<dyn TransportServer>) {}
    }

    #[test]
    fn test_transport_client_is_object_safe() {
        // This test compiles if TransportClient is object-safe
        fn _takes_boxed(_client: Box<dyn TransportClient>) {}
    }
}
