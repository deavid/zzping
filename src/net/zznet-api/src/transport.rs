//! Transport layer abstraction traits.
//!
//! These traits define the interface that all transport implementations must provide.
//! The transport layer is responsible for:
//! - Framing: Adding/removing length prefixes to delimit messages
//! - Network I/O: Actual sending/receiving of bytes
//! - Connection management: Establishing and tearing down connections
//!
//! The transport layer does NOT know about:
//! - Serialization: That's handled by the HELLO layer
//! - Protocol: That's handled by the HELLO and Session layers
//! - Application logic: That's handled by components

use crate::error::TransportError;
use async_trait::async_trait;
use bytes::Bytes;

/// A single bidirectional transport connection.
///
/// This trait represents an established connection that can send and receive
/// framed messages. Implementations handle the underlying I/O and framing,
/// presenting a simple bytes-in/bytes-out interface.
///
/// ## Framing Contract
///
/// All implementations MUST use the same framing protocol:
/// - Frame format: `[u32 BE length][payload bytes]`
/// - Length is 4 bytes, big-endian
/// - Maximum frame size: Implementations should enforce a limit (recommend 16 MiB)
/// - Zero-length frames are valid (can be used for heartbeats)
///
/// ## Identity and Security
///
/// All connections provide peer identity via `peer_identity()`. The identity
/// represents the cryptographic or claimed identity of the remote peer:
/// - TLS connections: Identity is cryptographically verified from certificates
/// - Raw TCP connections: Identity is claimed in HELLO messages (trust depends on application policy)
///
/// Applications should use `peer_identity()` for authorization decisions rather
/// than trusting network-layer claims alone.
///
/// ## Lifecycle
///
/// - Connection is assumed to be established when the trait object is created
/// - `recv()` returning `Ok(None)` indicates graceful close
/// - Errors indicate abnormal termination
/// - Dropping the connection should close it gracefully
///
/// ## Thread Safety
///
/// Implementations must be `Send` to allow passing between async tasks.
#[async_trait]
pub trait TransportConnection: Send {
    /// Sends a framed message over the connection.
    ///
    /// The implementation will:
    /// 1. Prepend a length header (u32 BE)
    /// 2. Write the complete frame to the underlying transport
    /// 3. Return when the write is complete (or has failed)
    ///
    /// Returns `Err` if the connection is closed or a network error occurs.
    async fn send(&mut self, frame: Bytes) -> Result<(), TransportError>;

    /// Receives a framed message from the connection.
    ///
    /// The implementation will:
    /// 1. Read the length header (u32 BE)
    /// 2. Read exactly that many bytes
    /// 3. Return the payload (without the length header)
    ///
    /// Returns:
    /// - `Ok(Some(frame))` when a complete frame is received
    /// - `Ok(None)` when the connection is closed gracefully
    /// - `Err` when a network error occurs
    ///
    /// This method will block until a complete frame is available or an error occurs.
    async fn recv(&mut self) -> Result<Option<Bytes>, TransportError>;

    /// Returns the peer address as a string for logging/metrics.
    ///
    /// Format is implementation-defined but should be human-readable.
    /// Examples: "192.168.1.100:5555", "mock:peer_a"
    ///
    /// Returns `None` if address information is unavailable.
    fn peer_addr(&self) -> Option<String>;

    /// Returns the verified identity of the peer from TLS certificate.
    ///
    /// Returns Some(identity) for TLS connections (extracted from CN + SAN).
    /// Returns None for raw TCP connections (no cryptographic identity available).
    ///
    /// The HELLO protocol provides role claims for both TLS and TCP modes.
    /// - For TLS: Use this identity to validate the HELLO role claim
    /// - For TCP: No identity to validate against (insecure mode required)
    fn peer_tls_identity(&self) -> Option<crate::types::PeerTLSIdentity>;
}

/// A transport server that accepts incoming connections.
///
/// This trait represents a server listening for connections. Each call to
/// `accept()` waits for and returns a new established connection.
///
/// ## Lifecycle
///
/// - Server is assumed to be listening when the trait object is created
/// - `accept()` should block until a connection arrives
/// - Dropping the server should stop listening
///
/// ## Error Handling
///
/// Individual connection errors (e.g., failed TLS handshake) should be
/// logged but not returned - the server should continue accepting.
/// Only fatal errors (e.g., socket bind failed) should be returned.
#[async_trait]
pub trait TransportServer: Send {
    /// Accepts a new incoming connection.
    ///
    /// This method blocks until a client connects and the connection is
    /// fully established (including any TLS handshake).
    ///
    /// Returns:
    /// - `Ok(connection)` when a new connection is accepted
    /// - `Err` only for fatal errors that prevent further accepts
    ///
    /// Implementation note: Transient errors (failed TLS handshake, invalid
    /// client certificate, etc.) should be logged and handled internally,
    /// not returned to the caller.
    async fn accept(&mut self) -> Result<Box<dyn TransportConnection>, TransportError>;
}

/// A transport client that creates outgoing connections.
///
/// This trait represents a client that can establish connections to a server.
/// Unlike `TransportServer`, clients are typically not "started" - they create
/// connections on-demand.
///
/// ## Lifecycle
///
/// - Client configuration is provided when the trait object is created
/// - `connect()` can be called multiple times to create connections
/// - Dropping the client is always safe (no active resources)
///
/// ## Reconnection
///
/// This trait does NOT handle automatic reconnection - that's the responsibility
/// of higher layers. Each `connect()` call is independent.
#[async_trait]
pub trait TransportClient: Send {
    /// Establishes a new connection to the configured server.
    ///
    /// This method blocks until the connection is fully established (including
    /// any TLS handshake) or fails.
    ///
    /// Returns:
    /// - `Ok(connection)` when successfully connected
    /// - `Err` if connection fails for any reason
    ///
    /// Multiple addresses: If the client is configured with multiple addresses,
    /// implementations should try them in order and return the first successful
    /// connection.
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
