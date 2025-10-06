//! Error types for transport layer operations.

use thiserror::Error;

/// Errors that can occur during transport operations.
///
/// All transport implementations must map their internal errors to these
/// types to provide a consistent error interface across different transports.
#[derive(Error, Debug)]
pub enum TransportError {
    /// An I/O error occurred during transport operations.
    ///
    /// This includes network errors, socket errors, or any other I/O failures.
    #[error("I/O error: {0}")]
    IoError(String),

    /// The connection was closed gracefully by the remote peer.
    ///
    /// This is NOT an error condition - it's the expected way connections end.
    /// Receivers should handle this by cleaning up connection state.
    #[error("Connection closed")]
    ConnectionClosed,

    /// An operation timed out.
    ///
    /// This can occur during connect, read, or write operations when the
    /// configured timeout is exceeded.
    #[error("Operation timed out")]
    Timeout,

    /// A frame exceeded the maximum allowed size.
    ///
    /// To prevent memory exhaustion attacks, frames are limited in size.
    /// The recommended limit is 16 MiB.
    #[error("Frame too large: {size} bytes (limit: {limit} bytes)")]
    FrameTooLarge {
        /// The size of the frame that was rejected.
        size: usize,
        /// The configured maximum frame size.
        limit: usize,
    },

    /// An error occurred during serialization or deserialization.
    ///
    /// This should be rare since serialization happens at a higher layer,
    /// but can occur if frame headers are malformed.
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// The transport is in an invalid state for the requested operation.
    ///
    /// For example, trying to send on a closed connection or accept on a
    /// stopped server.
    #[error("Invalid state: {0}")]
    InvalidState(String),
}

impl From<std::io::Error> for TransportError {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        match err.kind() {
            ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
            | ErrorKind::UnexpectedEof => TransportError::ConnectionClosed,
            ErrorKind::TimedOut => TransportError::Timeout,
            _ => TransportError::IoError(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = TransportError::IoError("network unreachable".to_string());
        assert_eq!(err.to_string(), "I/O error: network unreachable");

        let err = TransportError::ConnectionClosed;
        assert_eq!(err.to_string(), "Connection closed");

        let err = TransportError::FrameTooLarge {
            size: 20_000_000,
            limit: 16_777_216,
        };
        assert!(err.to_string().contains("20000000"));
        assert!(err.to_string().contains("16777216"));
    }

    #[test]
    fn test_io_error_conversion() {
        use std::io::{Error, ErrorKind};

        let io_err = Error::new(ErrorKind::ConnectionReset, "reset");
        let transport_err: TransportError = io_err.into();
        assert!(matches!(transport_err, TransportError::ConnectionClosed));

        let io_err = Error::new(ErrorKind::TimedOut, "timeout");
        let transport_err: TransportError = io_err.into();
        assert!(matches!(transport_err, TransportError::Timeout));

        let io_err = Error::new(ErrorKind::PermissionDenied, "denied");
        let transport_err: TransportError = io_err.into();
        assert!(matches!(transport_err, TransportError::IoError(_)));
    }
}
