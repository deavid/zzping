//! Configuration structures and loading.

use serde::{Deserialize, Serialize};

/// Collector application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorConfig {
    /// Unique identifier for this collector instance
    pub collector_id: String,

    /// Database connection settings
    pub database_host: String,
    pub database_port: u16,

    /// TLS configuration for mTLS connection
    pub tls: TlsConfig,

    /// Component-specific settings
    pub components: ComponentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    /// CA certificate for verifying server (database)
    pub ca_cert_path: String,
    /// Client certificate (this collector's identity)
    pub client_cert_path: String,
    /// Client private key
    pub client_key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentConfig {
    /// Heartbeat interval in seconds for collector state
    pub heartbeat_interval_secs: u64,

    /// Batch size for mem-db
    pub memdb_batch_size: usize,
}

impl CollectorConfig {
    /// Load configuration from a RON file.
    ///
    /// # Errors
    /// Returns error if file cannot be read or parsed.
    pub fn load(path: &str) -> crate::error::Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::error::CollectorError::Config(format!(
                "Failed to read config file {}: {}",
                path, e
            ))
        })?;

        let config: Self = ron::from_str(&content).map_err(|e| {
            crate::error::CollectorError::Config(format!("Failed to parse config: {}", e))
        })?;

        Ok(config)
    }

    /// Validate configuration values.
    ///
    /// # Errors
    /// Returns error if configuration has invalid values.
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.collector_id.is_empty() {
            return Err(crate::error::CollectorError::Config(
                "collector_id cannot be empty".into(),
            ));
        }

        if self.components.heartbeat_interval_secs == 0 {
            return Err(crate::error::CollectorError::Config(
                "heartbeat_interval_secs cannot be 0".into(),
            ));
        }

        if self.database_host.is_empty() {
            return Err(crate::error::CollectorError::Config(
                "database_host cannot be empty".into(),
            ));
        }

        if self.database_port == 0 {
            return Err(crate::error::CollectorError::Config(
                "database_port cannot be 0".into(),
            ));
        }

        // Validate TLS file paths exist
        if !std::path::Path::new(&self.tls.ca_cert_path).exists() {
            return Err(crate::error::CollectorError::Config(format!(
                "CA certificate not found: {}",
                self.tls.ca_cert_path
            )));
        }

        if !std::path::Path::new(&self.tls.client_cert_path).exists() {
            return Err(crate::error::CollectorError::Config(format!(
                "Client certificate not found: {}",
                self.tls.client_cert_path
            )));
        }

        if !std::path::Path::new(&self.tls.client_key_path).exists() {
            return Err(crate::error::CollectorError::Config(format!(
                "Client private key not found: {}",
                self.tls.client_key_path
            )));
        }

        Ok(())
    }
}
