//! Error types for zznet-app-utils.

use thiserror::Error;

/// Errors that can occur when using zznet-app-utils.
#[derive(Error, Debug)]
pub enum Error {
    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// TLS error.
    #[error("TLS error: {0}")]
    Tls(String),

    /// I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// RON deserialization error.
    #[error("RON parse error: {0}")]
    Ron(#[from] ron::error::SpannedError),

    /// Rustls error.
    #[error("Rustls error: {0}")]
    Rustls(String),
}

/// Result type alias for zznet-app-utils operations.
pub type Result<T> = std::result::Result<T, Error>;
