//! Configuration structures and loading.

use serde::{Deserialize, Serialize};

/// Database application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Network binding settings
    pub bind_host: String,
    pub bind_port: u16,

    /// TLS configuration for mTLS server (optional for TCP-only mode)
    pub tls: Option<TlsConfig>,

    /// Component-specific settings
    pub components: ComponentConfig,
    /// Data working directory for runtime files (e.g., intent.ron).
    ///
    /// This field is mandatory. If a relative path is provided it is resolved
    /// relative to the directory containing the RON file. If you want the
    /// same directory as the RON file, set `data_dir: "."` explicitly.
    pub data_dir: String,
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

    /// Timeout in milliseconds for reading a single message frame from a connection.
    /// If no data is received within this time, the connection is closed.
    /// Defaults to 500ms. Set to 0 to disable timeout (not recommended).
    #[serde(default = "default_message_frame_timeout_ms")]
    pub message_frame_timeout_ms: u64,
}

impl ComponentConfig {
    /// Create component configuration with faster timing suitable for testing/demos.
    ///
    /// Uses shorter intervals than production defaults:
    /// - Stale timeout: 1s instead of 30s
    /// - Frame timeout: 100ms instead of 500ms
    /// - Max collectors: 10 instead of 100
    ///
    /// # Example
    /// ```ignore
    /// let config = ComponentConfig::fast_timing();
    /// assert_eq!(config.stale_timeout_secs, 1);
    /// ```
    pub fn fast_timing() -> Self {
        Self {
            stale_timeout_secs: 1,
            max_collectors: 10,
            message_frame_timeout_ms: 100,
        }
    }
}

fn default_message_frame_timeout_ms() -> u64 {
    500
}

impl DatabaseConfig {
    /// Create a minimal configuration suitable for testing, demos, or development.
    ///
    /// This configuration uses:
    /// - TCP-only (no TLS)
    /// - localhost binding
    /// - OS-assigned port (port 0) for parallel tests
    /// - Fast timing intervals for testing
    /// - Minimal resource usage
    /// - Current directory for data storage
    ///
    /// # Example
    /// ```ignore
    /// use zzping_database::config::DatabaseConfig;
    ///
    /// let config = DatabaseConfig::for_testing();
    /// assert!(config.tls.is_none()); // No TLS in test mode
    /// assert_eq!(config.bind_port, 0); // OS assigns port
    /// ```
    pub fn for_testing() -> Self {
        Self {
            bind_host: "127.0.0.1".into(),
            bind_port: 0, // OS assigns port (useful for parallel tests)
            tls: None,    // TCP-only
            components: ComponentConfig::fast_timing(),
            data_dir: ".".into(),
        }
    }

    /// Load configuration from a RON file.
    ///
    /// Reads and parses the RON configuration file. Fails if the file
    /// cannot be read or contains invalid RON syntax.
    pub fn load(path: &str) -> crate::error::Result<Self> {
        // Read the RON file contents
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::error::DatabaseError::Config(format!(
                "Failed to read config file {}: {}",
                path, e
            ))
        })?;

        let config: Self = ron::from_str(&content).map_err(|e| {
            crate::error::DatabaseError::Config(format!("Failed to parse config: {}", e))
        })?;

        // Resolve relative paths relative to the config file directory.
        // This makes paths in the RON file behave intuitively: relative paths
        // are interpreted relative to the config file location, not the CWD.
        let config_file_dir = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        // Helper to resolve a single path string
        let resolve = |p: &str| {
            let pb = std::path::Path::new(p);
            if pb.is_relative() {
                config_file_dir.join(pb).to_string_lossy().to_string()
            } else {
                p.to_string()
            }
        };

        // Resolve CA certs and server cert/key paths (if TLS enabled)
        let mut resolved = config.clone();
        if let Some(tls) = &mut resolved.tls {
            tls.ca_cert_paths = tls.ca_cert_paths.iter().map(|p| resolve(p)).collect();
            tls.server_cert_path = resolve(&tls.server_cert_path);
            tls.server_key_path = resolve(&tls.server_key_path);
        }

        // Resolve data_dir (mandatory) relative to the config file directory
        let d = resolved.data_dir.clone();
        let dpath = std::path::Path::new(&d);
        resolved.data_dir = if dpath.is_relative() {
            config_file_dir.join(dpath).to_string_lossy().to_string()
        } else {
            d
        };

        Ok(resolved)
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

        // message_frame_timeout_ms can be 0 to disable (though not recommended),
        // but we don't validate it here to allow that option if needed

        if self.data_dir.is_empty() {
            return Err(crate::error::DatabaseError::Config(
                "data_dir cannot be empty; set to '.' to use config directory".into(),
            ));
        }

        // Validate TLS file paths exist (only if TLS is enabled)
        if let Some(tls) = &self.tls {
            if tls.ca_cert_paths.is_empty() {
                return Err(crate::error::DatabaseError::Config(
                    "At least one CA certificate path is required".into(),
                ));
            }

            for ca_path in &tls.ca_cert_paths {
                if !std::path::Path::new(ca_path).exists() {
                    return Err(crate::error::DatabaseError::Config(format!(
                        "CA certificate not found: {}",
                        ca_path
                    )));
                }
            }

            if !std::path::Path::new(&tls.server_cert_path).exists() {
                return Err(crate::error::DatabaseError::Config(format!(
                    "Server certificate not found: {}",
                    tls.server_cert_path
                )));
            }

            if !std::path::Path::new(&tls.server_key_path).exists() {
                return Err(crate::error::DatabaseError::Config(format!(
                    "Server private key not found: {}",
                    tls.server_key_path
                )));
            }
        } else {
            tracing::warn!("Running without TLS - connections will use plain TCP");
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
            tls: Some(TlsConfig {
                ca_cert_paths: vec![certs_dir.join("ca.pem").to_str().unwrap().to_string()],
                server_cert_path: certs_dir.join("database.pem").to_str().unwrap().to_string(),
                server_key_path: certs_dir.join("database.key").to_str().unwrap().to_string(),
            }),
            components: ComponentConfig {
                stale_timeout_secs: 30,
                max_collectors: 100,
                message_frame_timeout_ms: 500,
            },
            data_dir: String::from("."),
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
        tls: Some(TlsConfig(
            ca_cert_paths: ["{}"],
            server_cert_path: "{}",
            server_key_path: "{}",
        )),
        data_dir: ".",
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
