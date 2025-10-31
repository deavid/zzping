//! Configuration structures and loading.

use serde::{Deserialize, Serialize};
use zznet_builder::traits::ZZNetConfig;

use crate::error::CollectorError;

/// Collector application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorConfig {
    /// Unique identifier for this collector instance
    pub collector_id: String,

    /// Database connection settings
    pub database_host: String,
    /// Database connection port.
    pub database_port: u16,

    /// TLS configuration for mTLS connection (optional for TCP-only mode)
    pub tls: Option<CollectorTlsConfig>,

    /// Component-specific settings
    pub components: ComponentConfig,

    /// Delay in milliseconds between reconnection attempts (default: 5000ms)
    #[serde(default = "default_reconnect_delay_ms")]
    pub reconnect_delay_ms: u64,
}

fn default_reconnect_delay_ms() -> u64 {
    5000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// TLS configuration for mTLS.
pub struct CollectorTlsConfig {
    /// CA certificate for verifying server (database)
    pub ca_cert_path: String,
    /// Client certificate (this collector's identity)
    pub client_cert_path: String,
    /// Client private key
    pub client_key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Component-specific configuration.
pub struct ComponentConfig {
    /// Heartbeat interval in milliseconds for collector state
    pub heartbeat_interval_ms: u64,

    /// Batch size for mem-db
    pub memdb_batch_size: usize,
}

impl ComponentConfig {
    /// Create component configuration with faster timing suitable for testing/demos.
    ///
    /// Uses shorter intervals than production defaults:
    /// - Heartbeat: 100ms instead of 5000ms
    /// - Batch size: 5 instead of 50
    pub fn fast_timing() -> Self {
        Self {
            heartbeat_interval_ms: 100,
            memdb_batch_size: 5,
        }
    }
}

impl CollectorConfig {
    /// Create a minimal configuration suitable for testing, demos, or development.
    ///
    /// This configuration uses:
    /// - TCP-only (no TLS)
    /// - localhost database connection
    /// - Fast timing intervals for testing
    /// - Minimal resource usage
    pub fn for_testing(collector_id: impl Into<String>) -> Self {
        Self {
            collector_id: collector_id.into(),
            database_host: "127.0.0.1".into(),
            database_port: 58443,
            tls: None, // TCP-only
            components: ComponentConfig::fast_timing(),
            reconnect_delay_ms: 100, // Fast reconnect for testing
        }
    }
}

/// Implement ZZNetConfig trait for CollectorConfig
impl ZZNetConfig for CollectorConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.collector_id.is_empty() {
            Err(CollectorError::Config(
                "collector_id cannot be empty".into(),
            ))?;
        }

        if self.components.heartbeat_interval_ms == 0 {
            Err(CollectorError::Config(
                "heartbeat_interval_ms cannot be 0".into(),
            ))?;
        }

        if self.database_host.is_empty() {
            Err(CollectorError::Config(
                "database_host cannot be empty".into(),
            ))?;
        }

        if self.database_port == 0 {
            Err(CollectorError::Config("database_port cannot be 0".into()))?;
        }

        // Validate TLS file paths exist (only if TLS is enabled)
        if let Some(tls) = &self.tls {
            if !std::path::Path::new(&tls.ca_cert_path).exists() {
                Err(CollectorError::Config(format!(
                    "CA certificate not found: {}",
                    tls.ca_cert_path
                )))?;
            }

            if !std::path::Path::new(&tls.client_cert_path).exists() {
                Err(CollectorError::Config(format!(
                    "Client certificate not found: {}",
                    tls.client_cert_path
                )))?;
            }

            if !std::path::Path::new(&tls.client_key_path).exists() {
                Err(CollectorError::Config(format!(
                    "Client private key not found: {}",
                    tls.client_key_path
                )))?;
            }
        } else {
            tracing::warn!("Running without TLS - connections will use plain TCP");
        }

        Ok(())
    }

    fn log_startup_info(&self) {
        tracing::info!("Collector ID: {}", self.collector_id);
        tracing::info!("Database: {}:{}", self.database_host, self.database_port);
        if self.tls.is_some() {
            tracing::info!("TLS: enabled (mTLS connection)");
        } else {
            tracing::warn!("TLS: disabled (plain TCP connection)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use tempfile::NamedTempFile;
    use zznet_builder::config::load_ron_config;

    fn create_test_config() -> CollectorConfig {
        let certs_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("test_certs");
        let ca_cert_path = certs_dir.join("ca.pem").to_string_lossy().to_string();
        let client_cert_path = certs_dir
            .join("collector.pem")
            .to_string_lossy()
            .to_string();
        let client_key_path = certs_dir
            .join("collector.key")
            .to_string_lossy()
            .to_string();
        CollectorConfig {
            collector_id: "test-collector".into(),
            database_host: "localhost".into(),
            database_port: 8443,
            components: ComponentConfig {
                heartbeat_interval_ms: 5000,
                memdb_batch_size: 50,
            },
            tls: Some(CollectorTlsConfig {
                ca_cert_path,
                client_cert_path,
                client_key_path,
            }),
            reconnect_delay_ms: 5000,
        }
    }

    #[test]
    fn test_valid_config_validates() {
        let config = create_test_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_collector_id_fails_validation() {
        let mut config = create_test_config();
        config.collector_id = String::new();

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("collector_id"));
    }

    #[test]
    fn test_zero_port_fails_validation() {
        let mut config = create_test_config();
        config.database_port = 0;

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("port"));
    }

    #[test]
    fn test_zero_heartbeat_interval_fails_validation() {
        let mut config = create_test_config();
        config.components.heartbeat_interval_ms = 0;

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("heartbeat"));
    }

    #[test]
    fn test_load_valid_config_file() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let certs_dir = workspace_root.join("test_certs");
        let config_content = format!(
            r#"
    CollectorConfig(
        collector_id: "test-collector",
        database_host: "127.0.0.1",
        database_port: 8443,
        tls: Some(CollectorTlsConfig(
            ca_cert_path: "{}",
            client_cert_path: "{}",
            client_key_path: "{}",
        )),
        components: ComponentConfig(
            heartbeat_interval_ms: 5000,
            memdb_batch_size: 100,
        ),
    )
    "#,
            certs_dir.join("ca.pem").to_str().unwrap(),
            certs_dir.join("collector.pem").to_str().unwrap(),
            certs_dir.join("collector.key").to_str().unwrap()
        );

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(config_content.as_bytes()).unwrap();
        let path = temp_file.path().to_str().unwrap();

        let config = load_ron_config::<CollectorConfig>(path).expect("Failed to load config");
        assert_eq!(config.collector_id, "test-collector");
        assert_eq!(config.database_host, "127.0.0.1");
        assert_eq!(config.database_port, 8443);
    }

    #[test]
    fn test_load_nonexistent_file_fails() {
        let result = load_ron_config::<CollectorConfig>("/nonexistent/path/config.ron");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to read"));
    }

    #[test]
    fn test_load_invalid_ron_fails() {
        let invalid_content = "this is not valid RON {{{";

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(invalid_content.as_bytes()).unwrap();
        let path = temp_file.path().to_str().unwrap();

        let result = load_ron_config::<CollectorConfig>(path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to parse"));
    }
}
