//! Error types for transport layer operations.

use thiserror::Error;

/// Wrapper to group IO Errors for common usage - Connection closed and time out.
#[derive(Error, Debug)]
pub enum TransportError {
    /// An I/O error occurred during transport operations.
    #[error("I/O error: {0}")]
    IoError(std::io::Error),

    /// The connection was closed gracefully by the remote peer.
    #[error("Connection closed: {0}")]
    ConnectionClosed(std::io::Error),

    /// An operation timed out.
    #[error("Operation timed out: {0}")]
    Timeout(std::io::Error),
}

impl From<std::io::Error> for TransportError {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        match err.kind() {
            // Connection Closed errors:
            ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
            | ErrorKind::UnexpectedEof => TransportError::ConnectionClosed(err),
            // Time-out errors:
            ErrorKind::TimedOut | ErrorKind::WouldBlock => TransportError::Timeout(err),

            // Remaining errors:
            _ => TransportError::IoError(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        use std::io::Error;
        let err = TransportError::IoError(Error::other("network unreachable"));
        assert_eq!(err.to_string(), "I/O error: network unreachable");

        let err = TransportError::ConnectionClosed(Error::other("other"));
        assert_eq!(err.to_string(), "Connection closed: other");
    }

    #[test]
    fn test_io_error_conversion() {
        use std::io::{Error, ErrorKind};

        let io_err = Error::new(ErrorKind::ConnectionReset, "reset");
        let transport_err: TransportError = io_err.into();
        assert!(matches!(transport_err, TransportError::ConnectionClosed(_)));

        let io_err = Error::new(ErrorKind::TimedOut, "timeout");
        let transport_err: TransportError = io_err.into();
        assert!(matches!(transport_err, TransportError::Timeout(_)));

        let io_err = Error::new(ErrorKind::PermissionDenied, "denied");
        let transport_err: TransportError = io_err.into();
        assert!(matches!(transport_err, TransportError::IoError(_)));
    }
}
