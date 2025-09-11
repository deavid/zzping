//! Handles loading the database's "intent" configuration file.
//!
//! This file, typically `intent.ron`, defines the desired state for all connected
//! collectors, such as which targets they should ping and at what rate.

use serde::Deserialize;
use std::fs;

/// Represents the database's intended configuration for all collectors.
///
/// This struct is deserialized from a RON file (`intent.ron`). The database
/// service reads this file at startup and sends this configuration data to
/// collectors in every `HeartbeatResponse`. This allows for dynamic, centralized
/// control over the entire fleet of collectors.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IntentConfig {
    /// The number of pings per second each collector should attempt to send to
    /// each target.
    pub ping_rate_pps: u64,
    /// A list of IP addresses or hostnames that should be pinged.
    pub targets: Vec<String>,
}

/// Loads and deserializes the `IntentConfig` from a given file path.
///
/// The configuration is expected to be in the RON (Rusty Object Notation) format,
/// which is a human-friendly subset of Rust's struct syntax.
pub fn load_intent_config(path: &str) -> anyhow::Result<IntentConfig> {
    let content = fs::read_to_string(path)?;
    let config: IntentConfig = ron::from_str(&content)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Removed use ntest::timeout;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    // Removed #[timeout(100)]
    fn test_load_intent_config() {
        let content = r#"
(
    ping_rate_pps: 30,
    targets: [
        "8.8.8.8",
        "1.1.1.1",
    ],
)
"#;
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{content}").unwrap();

        let config = load_intent_config(file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.ping_rate_pps, 30);
        assert_eq!(config.targets, vec!["8.8.8.8", "1.1.1.1"]);
    }

    #[test]
    // Removed #[timeout(100)]
    fn test_load_intent_config_file_not_found() {
        let result = load_intent_config("non_existent_file.ron");
        assert!(result.is_err());
    }

    #[test]
    // Removed #[timeout(100)]
    fn test_load_intent_config_invalid_ron() {
        let content = r#"
(
    ping_rate_pps 30, // Missing colon
    targets: [
        "8.8.8.8",
        "1.1.1.1",
    ],
)
"#;
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{content}").unwrap();

        let result = load_intent_config(file.path().to_str().unwrap());
        assert!(result.is_err());
    }
}