//! Error types for the zznet-connection crate.

use thiserror::Error;

/// Comprehensive error type for all zznet-connection operations.
#[derive(Debug, Error)]
pub enum ZzNetConnectionError {
    #[error("Serialization failed: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    #[error("Invalid protocol state: {0}")]
    InvalidState(String),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Room access denied: {room} for role {role:?}")]
    RoomAccessDenied { room: String, role: crate::auth::AuthRole },

    #[error("Frame too large: {0} bytes")]
    FrameTooLarge(usize),

    #[error("Invalid frame format")]
    InvalidFrameFormat,
}

pub type Result<T> = std::result::Result<T, ZzNetConnectionError>;