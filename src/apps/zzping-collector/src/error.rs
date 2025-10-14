//! Error types for the collector application.

use thiserror::Error;

/// Errors that can occur in the collector application.
#[derive(Error, Debug)]
pub enum CollectorError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Service error: {0}")]
    Service(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Component error: {0}")]
    Component(String),

    #[error("Pinger error: {0}")]
    Pinger(#[from] zzpinger::error::PingerError),
}

/// Result type alias for collector operations.
pub type Result<T> = std::result::Result<T, CollectorError>;
