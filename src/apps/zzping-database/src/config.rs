//! Configuration structures and loading.

use serde::{Deserialize, Serialize};

/// Database application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Network binding settings
    pub bind_host: String,
    pub bind_port: u16,

    /// TLS configuration for mTLS server
    pub tls: TlsConfig,

    /// Component-specific settings
    pub components: ComponentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    /// CA certificate for verifying client certificates (from collectors)
    pub ca_cert_path: String,
    /// Server certificate (this database's identity)
    pub server_cert_path: String,
    /// Server private key
    pub server_key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentConfig {
    /// Heartbeat timeout in seconds for collector state tracking
    pub stale_timeout_secs: u64,

    /// Maximum number of collectors to accept
    pub max_collectors: usize,
}

impl DatabaseConfig {
    /// Load configuration from a RON file.
    ///
    /// Reads and parses the RON configuration file. Fails if the file
    /// cannot be read or contains invalid RON syntax.
    pub fn load(path: &str) -> crate::error::Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::error::DatabaseError::Config(format!(
                "Failed to read config file {}: {}",
                path, e
            ))
        })?;

        let config: Self = ron::from_str(&content).map_err(|e| {
            crate::error::DatabaseError::Config(format!("Failed to parse config: {}", e))
        })?;

        Ok(config)
    }

    /// Validate configuration values.
    ///
    /// Checks all configuration values for validity. Ensures required fields
    /// are not empty, numeric values are in acceptable ranges, and file paths
    /// point to existing files.
    ///
    /// Fails if any validation check does not pass.
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.bind_host.is_empty() {
            return Err(crate::error::DatabaseError::Config(
                "bind_host cannot be empty".into(),
            ));
        }

        if self.bind_port == 0 {
            return Err(crate::error::DatabaseError::Config(
                "bind_port cannot be 0".into(),
            ));
        }

        if self.components.stale_timeout_secs == 0 {
            return Err(crate::error::DatabaseError::Config(
                "stale_timeout_secs cannot be 0".into(),
            ));
        }

        if self.components.max_collectors == 0 {
            return Err(crate::error::DatabaseError::Config(
                "max_collectors cannot be 0".into(),
            ));
        }

        // Validate TLS file paths exist
        if !std::path::Path::new(&self.tls.ca_cert_path).exists() {
            return Err(crate::error::DatabaseError::Config(format!(
                "CA certificate not found: {}",
                self.tls.ca_cert_path
            )));
        }

        if !std::path::Path::new(&self.tls.server_cert_path).exists() {
            return Err(crate::error::DatabaseError::Config(format!(
                "Server certificate not found: {}",
                self.tls.server_cert_path
            )));
        }

        if !std::path::Path::new(&self.tls.server_key_path).exists() {
            return Err(crate::error::DatabaseError::Config(format!(
                "Server private key not found: {}",
                self.tls.server_key_path
            )));
        }

        Ok(())
    }
}
