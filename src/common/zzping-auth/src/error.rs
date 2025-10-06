//! Authorization error types

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during authorization operations
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Identity not in allow-list: {0}")]
    /// Peer identity is not present in the configured allow-list.
    IdentityNotAllowed(String),

    #[error("Unknown role: {0}")]
    /// Role name from certificate is not recognized by the system.
    UnknownRole(String),

    #[error("TLS is required but connection is not encrypted")]
    /// A secure (TLS) connection is required for this operation.
    TlsRequired,

    #[error("No identity available")]
    /// No client identity could be extracted from the connection.
    NoIdentityAvailable,

    #[error("Certificate error: {0}")]
    /// Generic certificate validation error.
    CertificateError(String),

    #[error("Certificate expired")]
    /// Peer certificate is expired.
    CertificateExpired,

    #[error("Certificate not yet valid")]
    /// Peer certificate is not yet valid.
    CertificateNotYetValid,

    #[error("Configuration file error: {path:?}: {source}")]
    /// Failure reading or opening the authorization configuration file.
    ConfigFileError {
        /// Path to the config file that failed.
        path: PathBuf,
        #[source]
        /// Underlying I/O error.
        source: std::io::Error,
    },

    #[error("Configuration error: {0}")]
    /// Initial validation or semantic error in the config contents.
    ConfigError(String),

    #[error("I/O error: {0}")]
    /// I/O error wrapper.
    Io(#[from] std::io::Error),

    #[error("TOML parsing error: {0}")]
    /// Error parsing TOML configuration.
    Toml(#[from] toml::de::Error),
}
