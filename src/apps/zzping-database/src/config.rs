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
    /// CA certificates for verifying client certificates (from collectors)
    /// Multiple paths supported to allow certificate rotation (dual-CA)
    pub ca_cert_paths: Vec<String>,
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

        // Validate TLS file paths exist (support multiple CA certs)
        if self.tls.ca_cert_paths.is_empty() {
            return Err(crate::error::DatabaseError::Config(
                "At least one CA certificate path is required".into(),
            ));
        }

        for ca_path in &self.tls.ca_cert_paths {
            if !std::path::Path::new(ca_path).exists() {
                return Err(crate::error::DatabaseError::Config(format!(
                    "CA certificate not found: {}",
                    ca_path
                )));
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use tempfile::NamedTempFile;

    /// Helper to create a valid test configuration.
    fn create_valid_config() -> DatabaseConfig {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let certs_dir = workspace_root.join("test_certs");

        DatabaseConfig {
            bind_host: "0.0.0.0".into(),
            bind_port: 8443,
            tls: TlsConfig {
                ca_cert_paths: vec![certs_dir.join("ca.pem").to_str().unwrap().to_string()],
                server_cert_path: certs_dir.join("database.pem").to_str().unwrap().to_string(),
                server_key_path: certs_dir.join("database.key").to_str().unwrap().to_string(),
            },
            components: ComponentConfig {
                stale_timeout_secs: 30,
                max_collectors: 100,
            },
        }
    }

    #[test]
    fn test_valid_config_validates() {
        let config = create_valid_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_bind_host_fails_validation() {
        let mut config = create_valid_config();
        config.bind_host = String::new();

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("bind_host"));
    }

    #[test]
    fn test_zero_port_fails_validation() {
        let mut config = create_valid_config();
        config.bind_port = 0;

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("port"));
    }

    #[test]
    fn test_zero_stale_timeout_fails_validation() {
        let mut config = create_valid_config();
        config.components.stale_timeout_secs = 0;

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("stale_timeout"));
    }

    #[test]
    fn test_zero_max_collectors_fails_validation() {
        let mut config = create_valid_config();
        config.components.max_collectors = 0;

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("max_collectors"));
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
    DatabaseConfig(
        bind_host: "0.0.0.0",
        bind_port: 8443,
        tls: TlsConfig(
            ca_cert_paths: ["{}"],
            server_cert_path: "{}",
            server_key_path: "{}",
        ),
        components: ComponentConfig(
            stale_timeout_secs: 30,
            max_collectors: 100,
        ),
    )
    "#,
            certs_dir.join("ca.pem").to_str().unwrap(),
            certs_dir.join("database.pem").to_str().unwrap(),
            certs_dir.join("database.key").to_str().unwrap()
        );

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(config_content.as_bytes()).unwrap();
        let path = temp_file.path().to_str().unwrap();

        let config = DatabaseConfig::load(path).expect("Failed to load config");
        assert_eq!(config.bind_host, "0.0.0.0");
        assert_eq!(config.bind_port, 8443);
    }

    #[test]
    fn test_load_nonexistent_file_fails() {
        let result = DatabaseConfig::load("/nonexistent/path/config.ron");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to read"));
    }

    #[test]
    fn test_load_invalid_ron_fails() {
        let invalid_content = "this is not valid RON {{{";

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(invalid_content.as_bytes()).unwrap();
        let path = temp_file.path().to_str().unwrap();

        let result = DatabaseConfig::load(path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to parse"));
    }
}
