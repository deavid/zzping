//! Authorization error types

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during authorization operations
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Identity not in allow-list: {0}")]
    IdentityNotAllowed(String),

    #[error("Unknown role: {0}")]
    UnknownRole(String),

    #[error("TLS is required but connection is not encrypted")]
    TlsRequired,

    #[error("No identity available")]
    NoIdentityAvailable,

    #[error("Certificate error: {0}")]
    CertificateError(String),

    #[error("Certificate expired")]
    CertificateExpired,

    #[error("Certificate not yet valid")]
    CertificateNotYetValid,

    #[error("Configuration file error: {path:?}: {source}")]
    ConfigFileError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parsing error: {0}")]
    Toml(#[from] toml::de::Error),
}
