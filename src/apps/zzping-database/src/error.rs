//! Error types for the database application.

use thiserror::Error;

/// Errors that can occur in the database application.
#[derive(Error, Debug)]
pub enum DatabaseError {
    /// Configuration validation, parsing, or file-related issues.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Errors raised by the service orchestration or runtime lifecycle.
    #[error("Service error: {0}")]
    Service(String),

    /// Underlying I/O failures (file reads, sockets, etc.).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Errors produced by embedded components/actors (e.g., db, memdb).
    #[error("Component error: {0}")]
    Component(String),

    /// TLS or certificate-related failures during startup or handshake.
    #[error("TLS error: {0}")]
    Tls(String),

    /// Persistence layer errors (database corruption, write failures).
    #[error("Persistence error: {0}")]
    Persistence(String),

    /// A single message frame timed out while reading from a peer.
    /// Used to detect unresponsive or misbehaving peers to protect resources.
    #[error("Message frame timeout: {0}")]
    MessageFrameTimeout(String),
}

/// Result type alias for database operations.
pub type Result<T> = std::result::Result<T, DatabaseError>;
