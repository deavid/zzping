//! Error types for the Pinger component.

use thiserror::Error;

/// Error type for pinger operations
#[derive(Error, Debug)]
pub enum PingerError {
    #[error("Invalid target configuration: {0}")]
    InvalidTarget(String),

    #[error("ICMP ping error: {0}")]
    PingError(String),

    #[error("Actor communication error: {0}")]
    ActorError(String),
}
