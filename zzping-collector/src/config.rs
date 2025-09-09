use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::path::Path;

use serde::Serialize;

/// The file-based configuration for the collector.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub collector_uuid: String,
    pub database_addr: String,
    pub auth_token: String,
    /// Test-only field: when true, use MockPingClient instead of real ping client
    #[serde(default)]
    pub use_mock_ping_client: bool,
}

impl Config {
    /// Loads the configuration from the specified file path.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the `collector.ron` configuration file.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config = ron::from_str(&content)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ntest::timeout;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    #[timeout(100)]
    fn test_load_valid_config() {
        let content = r#"
(
    collector_uuid: "a1b2c3d4-e5f6-7890-1234-567890abcdef",
    database_addr: "http://127.0.0.1:7878",
    auth_token: "my-secret-token",
)
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let config = Config::load(file.path());
        assert!(config.is_ok());
        let config = config.unwrap();

        assert_eq!(
            config.collector_uuid,
            "a1b2c3d4-e5f6-7890-1234-567890abcdef"
        );
        assert_eq!(config.database_addr, "http://127.0.0.1:7878");
        assert_eq!(config.auth_token, "my-secret-token");
    }

    #[test]
    #[timeout(100)]
    fn test_load_invalid_config() {
        let content = r#"
(
    collector_uuid: "a1b2c3d4-e5f6-7890-1234-567890abcdef",
    // database_addr is missing
    auth_token: "my-secret-token",
)
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let config = Config::load(file.path());
        assert!(config.is_err());
    }

    #[test]
    #[timeout(100)]
    fn test_load_nonexistent_file() {
        let config = Config::load("nonexistent-file.ron");
        assert!(config.is_err());
    }
}
