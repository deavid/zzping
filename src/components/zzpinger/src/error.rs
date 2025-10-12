//! Error types for the Pinger component.
//!
//! Defines specific error variants to enable precise error handling and user feedback.
//! Prevents silent failures by surfacing configuration and runtime issues.

use thiserror::Error;

/// Error type for pinger operations. Categorizes failures to guide error recovery.
/// Enables callers to distinguish between configuration errors, ping failures, and communication issues.
#[derive(Error, Debug)]
pub enum PingerError {
    /// Configuration validation failed. Prevents invalid targets that could cause hangs or crashes.
    #[error("Invalid target configuration: {0}")]
    InvalidTarget(String),

    /// ICMP ping operation failed. Indicates network or permission issues during ping attempts.
    #[error("ICMP ping error: {0}")]
    PingError(String),

    /// Actix actor communication failed. Signals internal messaging problems requiring restart.
    #[error("Actor communication error: {0}")]
    ActorError(String),
}
