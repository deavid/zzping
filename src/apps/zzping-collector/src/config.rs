//! Configuration structures and loading.

use serde::{Deserialize, Serialize};

/// Collector application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorConfig {
    /// Unique identifier for this collector instance
    pub collector_id: String,

    /// Database connection settings
    pub database_host: String,
    /// Database connection port.
    pub database_port: u16,

    /// TLS configuration for mTLS connection
    pub tls: TlsConfig,

    /// Component-specific settings
    pub components: ComponentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// TLS configuration for mTLS.
pub struct TlsConfig {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use tempfile::NamedTempFile;

    /// Helper to create a valid test configuration.
    fn create_valid_config() -> CollectorConfig {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let certs_dir = workspace_root.join("test_certs");

        CollectorConfig {
            collector_id: "test-collector-01".into(),
            database_host: "127.0.0.1".into(),
            database_port: 8443,
            tls: TlsConfig {
                ca_cert_path: certs_dir.join("ca.pem").to_str().unwrap().to_string(),
                client_cert_path: certs_dir
                    .join("collector.pem")
                    .to_str()
                    .unwrap()
                    .to_string(),
                client_key_path: certs_dir
                    .join("collector.key")
                    .to_str()
                    .unwrap()
                    .to_string(),
            },
            components: ComponentConfig {
                heartbeat_interval_secs: 5,
                memdb_batch_size: 100,
            },
        }
    }

    #[test]
    fn test_valid_config_validates() {
        let config = create_valid_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_collector_id_fails_validation() {
        let mut config = create_valid_config();
        config.collector_id = String::new();

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("collector_id"));
    }

    #[test]
    fn test_zero_port_fails_validation() {
        let mut config = create_valid_config();
        config.database_port = 0;

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("port"));
    }

    #[test]
    fn test_zero_heartbeat_interval_fails_validation() {
        let mut config = create_valid_config();
        config.components.heartbeat_interval_secs = 0;

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
        tls: TlsConfig(
            ca_cert_path: "{}",
            client_cert_path: "{}",
            client_key_path: "{}",
        ),
        components: ComponentConfig(
            heartbeat_interval_secs: 5,
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

        let config = CollectorConfig::load(path).expect("Failed to load config");
        assert_eq!(config.collector_id, "test-collector");
        assert_eq!(config.database_host, "127.0.0.1");
        assert_eq!(config.database_port, 8443);
    }

    #[test]
    fn test_load_nonexistent_file_fails() {
        let result = CollectorConfig::load("/nonexistent/path/config.ron");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to read"));
    }

    #[test]
    fn test_load_invalid_ron_fails() {
        let invalid_content = "this is not valid RON {{{";

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(invalid_content.as_bytes()).unwrap();
        let path = temp_file.path().to_str().unwrap();

        let result = CollectorConfig::load(path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to parse"));
    }
}
