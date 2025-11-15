//! Authorization error types

use thiserror::Error;

/// Errors that can occur during authorization operations
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Unknown role: {0}")]
    /// Role name from certificate is not recognized by the system.
    UnknownRole(String),
}
