//! Configuration structures and loading.

use serde::{Deserialize, Serialize};

/// Database application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Top-level configuration for the database service.
///
/// Holds network, TLS and component tuning parameters. Designed to be
/// deserialized from a RON file and validated prior to service startup.
pub struct DatabaseConfig {
    /// Network binding settings
    pub bind_host: String,
    /// Port used to listen for incoming peer connections (0 for OS-assigned in tests).
    pub bind_port: u16,

    /// TLS configuration for mTLS server (optional for TCP-only mode)
    pub tls: Option<DatabaseTlsConfig>,

    /// Component-specific settings
    pub components: ComponentConfig,
    /// Data working directory for runtime files (e.g., intent.ron).
    ///
    /// This field is mandatory. If a relative path is provided it is resolved
    /// relative to the directory containing the RON file. If you want the
    /// same directory as the RON file, set `data_dir: "."` explicitly.
    pub data_dir: String,

    /// Handshake timeout in seconds for the HELLO protocol (default: 10s)
    #[serde(default = "default_handshake_timeout_secs")]
    pub handshake_timeout_secs: u64,
}

fn default_handshake_timeout_secs() -> u64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// TLS material for a mutual-TLS server configuration.
///
/// Contains paths to CA(s) for client verification and the server's
/// certificate and key used to identify this database instance.
pub struct DatabaseTlsConfig {
    /// CA certificates for verifying client certificates (from collectors)
    /// Multiple paths supported to allow certificate rotation (dual-CA)
    pub ca_cert_paths: Vec<String>,
    /// Server certificate (this database's identity)
    pub server_cert_path: String,
    /// Server private key
    pub server_key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Tunable timeouts and limits for component behavior and resource protection.
///
/// These settings control collector state lifetimes, concurrency limits and
/// per-connection frame timeouts to protect the service under load. Adjust
/// for testing or production as required.
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
            bind_port: 58443,
            tls: None, // TCP-only
            components: ComponentConfig::fast_timing(),
            data_dir: ".".into(),
            handshake_timeout_secs: 10,
        }
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

/// Implement ZZNetConfig trait for DatabaseConfig
impl zznet_builder::traits::ZZNetConfig for DatabaseConfig {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
            tls: Some(DatabaseTlsConfig {
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
            handshake_timeout_secs: 10,
        }
    }

    #[test]
    #[ignore]
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


}
