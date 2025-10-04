//! Error types for zznet-builder

use std::fmt;
use std::io;

/// Result type for builder operations
pub type BuilderResult<T> = Result<T, BuilderError>;

/// Errors that can occur during server/client building
#[derive(Debug)]
pub enum BuilderError {
    /// Missing required configuration
    MissingConfig(String),

    /// Invalid configuration value
    InvalidConfig(String),

    /// I/O error
    Io(io::Error),

    /// TLS configuration error
    TlsError(String),

    /// Transport error
    TransportError(String),

    /// Connection failed
    ConnectionFailed(String),

    /// Bind failed
    BindFailed(String),
}

impl fmt::Display for BuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuilderError::MissingConfig(field) => {
                write!(f, "Missing required configuration: {}", field)
            }
            BuilderError::InvalidConfig(msg) => {
                write!(f, "Invalid configuration: {}", msg)
            }
            BuilderError::Io(e) => write!(f, "I/O error: {}", e),
            BuilderError::TlsError(msg) => write!(f, "TLS error: {}", msg),
            BuilderError::TransportError(msg) => write!(f, "Transport error: {}", msg),
            BuilderError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            BuilderError::BindFailed(msg) => write!(f, "Bind failed: {}", msg),
        }
    }
}

impl std::error::Error for BuilderError {}

impl From<io::Error> for BuilderError {
    fn from(e: io::Error) -> Self {
        BuilderError::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = BuilderError::MissingConfig("bind_addr".to_string());
        assert_eq!(err.to_string(), "Missing required configuration: bind_addr");

        let err = BuilderError::InvalidConfig("port must be > 0".to_string());
        assert_eq!(err.to_string(), "Invalid configuration: port must be > 0");
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let builder_err: BuilderError = io_err.into();
        assert!(matches!(builder_err, BuilderError::Io(_)));
    }
}
