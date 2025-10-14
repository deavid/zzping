//! Error types for the database application.

use thiserror::Error;

/// Errors that can occur in the database application.
#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Service error: {0}")]
    Service(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Component error: {0}")]
    Component(String),

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("Persistence error: {0}")]
    Persistence(String),
}

/// Result type alias for database operations.
pub type Result<T> = std::result::Result<T, DatabaseError>;
