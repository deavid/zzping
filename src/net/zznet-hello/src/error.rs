//! Errors that can occur during the HELLO protocol handshake.
//!
//! These errors are fatal and should result in connection termination.

use thiserror::Error;
use zznet_api::error::TransportError;

/// Represents failures in the HELLO handshake process.
#[derive(Error, Debug)]
pub(crate) enum HelloError {
    /// Wraps an error from the underlying transport layer (e.g., network I/O).
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    /// A message could not be serialized or deserialized, indicating a
    /// malformed frame or incompatible protocol version.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// A catch-all for handshake failures that don't fit other categories.
    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    /// The protocol state machine received an unexpected message.
    #[error("Invalid state: {0}")]
    InvalidState(String),
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
    fn test_transport_error_conversion() {
        let transport_err = TransportError::ConnectionClosed(std::io::Error::other("test"));
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
