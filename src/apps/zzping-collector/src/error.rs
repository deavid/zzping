//! Error types for the collector application.

use thiserror::Error;

/// Errors that can occur in the collector application.
#[derive(Error, Debug)]
pub enum CollectorError {
    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Service error.
    #[error("Service error: {0}")]
    Service(String),

    /// I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Component error.
    #[error("Component error: {0}")]
    Component(String),

    /// Pinger error.
    #[error("Pinger error: {0}")]
    Pinger(#[from] zzpinger::error::PingerError),

    /// Anyhow error wrapper
    #[error("Generic error: {0}")]
    Anyhow(#[from] anyhow::Error),
}
