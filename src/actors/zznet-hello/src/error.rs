//! Error types for HELLO protocol operations.

use thiserror::Error;
use zznet_api::error::TransportError;

/// Errors that can occur during HELLO protocol operations.
///
/// These errors represent failures in the handshake process or room communication
/// setup. Most of these are fatal and should result in connection termination.
#[derive(Error, Debug)]
pub enum HelloError {
    /// Transport layer error occurred.
    ///
    /// This wraps errors from the underlying transport (network I/O failures, etc.).
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    /// Serialization or deserialization failed.
    ///
    /// This indicates a malformed frame or incompatible protocol version.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Protocol version mismatch.
    ///
    /// The peer is using an incompatible protocol version.
    #[error("Protocol version mismatch: expected {expected}, got {actual}")]
    VersionMismatch {
        /// The protocol version we support.
        expected: String,
        /// The protocol version the peer sent.
        actual: String,
    },

    /// Authentication failed.
    ///
    /// The peer's role is not authorized to connect.
    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    /// No common rooms between peers.
    ///
    /// The intersection of offered rooms is empty, so there's nothing to communicate about.
    #[error("No common rooms: local offered {local:?}, peer offered {peer:?}")]
    NoCommonRooms {
        /// Rooms this side offers.
        local: Vec<String>,
        /// Rooms the peer offers.
        peer: Vec<String>,
    },

    /// Handshake failed for an unspecified reason.
    ///
    /// This is a catch-all for handshake failures that don't fit other categories.
    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    /// Invalid state transition.
    ///
    /// The protocol state machine received an unexpected message or operation.
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Session manager communication failed.
    ///
    /// Could not send message to or receive response from SessionManager.
    #[error("Session manager error: {0}")]
    SessionManager(String),
}

impl From<bincode::error::EncodeError> for HelloError {
    fn from(err: bincode::error::EncodeError) -> Self {
        HelloError::Serialization(err.to_string())
    }
}

impl From<bincode::error::DecodeError> for HelloError {
    fn from(err: bincode::error::DecodeError) -> Self {
        HelloError::Serialization(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = HelloError::VersionMismatch {
            expected: "1.0".to_string(),
            actual: "2.0".to_string(),
        };
        assert!(err.to_string().contains("1.0"));
        assert!(err.to_string().contains("2.0"));

        let err = HelloError::NoCommonRooms {
            local: vec!["memdb".to_string()],
            peer: vec!["query".to_string()],
        };
        assert!(err.to_string().contains("memdb"));
        assert!(err.to_string().contains("query"));
    }

    #[test]
    fn test_transport_error_conversion() {
        let transport_err = TransportError::ConnectionClosed;
        let hello_err: HelloError = transport_err.into();
        assert!(matches!(hello_err, HelloError::Transport(_)));
    }

    #[test]
    fn test_bincode_error_conversion() {
        // Create a bincode error by trying to deserialize invalid data
        let invalid_data = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let result: Result<String, bincode::error::DecodeError> =
            bincode::serde::decode_from_slice(&invalid_data, bincode::config::standard())
                .map(|(value, _)| value);
        let bincode_err = result.unwrap_err();

        let hello_err: HelloError = bincode_err.into();
        assert!(matches!(hello_err, HelloError::Serialization(_)));
    }
}
