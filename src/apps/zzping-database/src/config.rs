//! Configuration structures and loading.

use serde::{Deserialize, Serialize};

/// Database application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Top-level configuration for the database service.
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
/// TLS configuration for the server.
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
/// Timeouts and limits for component behavior.
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
    /// Creates configuration with millisecond intervals for unit testing.
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
    /// Creates a minimal TCP-only configuration for testing.
    pub fn for_testing() -> Self {
        Self {
            bind_host: "127.0.0.1".into(),
            bind_port: 0, // OS-assigned
            tls: None,    // TCP-only
            components: ComponentConfig::fast_timing(),
            data_dir: ".".into(),
            handshake_timeout_secs: 1,
        }
    }

    /// Checks all configuration values for validity.
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
