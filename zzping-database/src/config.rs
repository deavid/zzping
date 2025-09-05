use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IntentConfig {
    pub ping_rate_pps: u64,
    pub targets: Vec<String>,
}

pub fn load_intent_config(path: &str) -> anyhow::Result<IntentConfig> {
    let content = fs::read_to_string(path)?;
    let config: IntentConfig = ron::from_str(&content)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ntest::timeout;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    #[timeout(100)]
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
    #[timeout(100)]
    fn test_load_intent_config_file_not_found() {
        let result = load_intent_config("non_existent_file.ron");
        assert!(result.is_err());
    }

    #[test]
    #[timeout(100)]
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
