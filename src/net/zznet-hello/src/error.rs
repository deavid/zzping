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

impl From<rmp_serde::encode::Error> for HelloError {
    fn from(err: rmp_serde::encode::Error) -> Self {
        HelloError::Serialization(err.to_string())
    }
}

impl From<rmp_serde::decode::Error> for HelloError {
    fn from(err: rmp_serde::decode::Error) -> Self {
        HelloError::Serialization(err.to_string())
    }
}
