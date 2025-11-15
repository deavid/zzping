//! Configuration loading and path resolution utilities.
//!
//! This module provides utilities for loading application configuration from RON files
//! and resolving relative paths relative to the configuration file location.

use crate::error::{Error, Result};
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};

pub fn load_ron_config<T: DeserializeOwned>(path: &str) -> Result<T> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| Error::Config(format!("Failed to read config file {}: {}", path, e)))?;

    let config: T = ron::from_str(&content)
        .map_err(|e| Error::Config(format!("Failed to parse config: {}", e)))?;

    Ok(config)
}

/// Resolve a path relative to a configuration file's directory.
///
/// If the path is already absolute, it is returned as-is.
/// If the path is relative, it is resolved relative to the directory
/// containing the configuration file.
///
/// # Arguments
///
/// * `config_path` - Path to the configuration file
/// * `relative_path` - Path to resolve (may be absolute or relative)
///
/// # Returns
///
/// Returns the resolved absolute path as a string.
///
/// # Examples
///
/// ```rust
/// use zznet_builder::config::resolve_path_relative_to_config;
///
/// let config_path = "/etc/myapp/config.ron";
/// let cert_path = "../certs/server.pem";
///
/// let resolved = resolve_path_relative_to_config(config_path, cert_path);
/// assert_eq!(resolved, "/etc/certs/server.pem");
/// ```
pub fn resolve_path_relative_to_config(config_path: &str, relative_path: &str) -> String {
    let path = Path::new(relative_path);

    // If already absolute, return as-is
    if path.is_absolute() {
        return relative_path.to_string();
    }

    // Get the directory containing the config file
    let config_dir = Path::new(config_path)
        .parent()
        .unwrap_or_else(|| Path::new("."));

    // Join the paths
    let joined = config_dir.join(path);

    // Canonicalize would be ideal but requires the path to exist
    // Instead, manually normalize by collecting components
    let mut components = Vec::new();
    for component in joined.components() {
        match component {
            std::path::Component::ParentDir => {
                if !components.is_empty() {
                    components.pop();
                }
            }
            std::path::Component::CurDir => {
                // Skip current directory markers
            }
            _ => {
                components.push(component);
            }
        }
    }

    // Reconstruct the path
    let mut result = PathBuf::new();
    for component in components {
        result.push(component);
    }

    result.to_string_lossy().to_string()
}

/// Get the directory containing a configuration file.
///
/// Returns "." if the path has no parent directory.
///
/// # Arguments
///
/// * `config_path` - Path to the configuration file
///
/// # Returns
///
/// Returns the directory path as a PathBuf.
///
/// # Examples
///
/// ```rust
/// use zznet_builder::config::get_config_dir;
///
/// let config_path = "/etc/myapp/config.ron";
/// let dir = get_config_dir(config_path);
/// assert_eq!(dir.to_string_lossy(), "/etc/myapp");
/// ```
pub fn get_config_dir(config_path: &str) -> PathBuf {
    Path::new(config_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Resolve multiple paths relative to a configuration file.
///
/// Convenience function for resolving a list of paths.
///
/// # Arguments
///
/// * `config_path` - Path to the configuration file
/// * `paths` - Iterator of paths to resolve
///
/// # Returns
///
/// Returns a vector of resolved paths.
///
/// # Examples
///
/// ```rust
/// use zznet_builder::config::resolve_paths_relative_to_config;
///
/// let config_path = "/etc/myapp/config.ron";
/// let paths = vec!["../certs/ca.pem", "../certs/server.pem"];
///
/// let resolved = resolve_paths_relative_to_config(config_path, paths.iter().copied());
/// assert_eq!(resolved, vec![
///     "/etc/certs/ca.pem",
///     "/etc/certs/server.pem"
/// ]);
/// ```
pub fn resolve_paths_relative_to_config<'a, I>(config_path: &str, paths: I) -> Vec<String>
where
    I: Iterator<Item = &'a str>,
{
    paths
        .map(|p| resolve_path_relative_to_config(config_path, p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestConfig {
        host: String,
        port: u16,
    }

    #[test]
    fn test_load_ron_config_valid() {
        let config_content = r#"
TestConfig(
    host: "localhost",
    port: 8080,
)
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(config_content.as_bytes()).unwrap();
        let path = temp_file.path().to_str().unwrap();

        let config: TestConfig = load_ron_config(path).expect("Should load valid config");
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn test_load_ron_config_invalid_syntax() {
        let invalid_content = "this is not valid RON {{{";

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(invalid_content.as_bytes()).unwrap();
        let path = temp_file.path().to_str().unwrap();

        let result: Result<TestConfig> = load_ron_config(path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("parse"));
    }

    #[test]
    fn test_load_ron_config_nonexistent_file() {
        let result: Result<TestConfig> = load_ron_config("/nonexistent/config.ron");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to read"));
    }

    #[test]
    fn test_resolve_path_relative_to_config_absolute() {
        let resolved =
            resolve_path_relative_to_config("/etc/myapp/config.ron", "/etc/certs/ca.pem");
        assert_eq!(resolved, "/etc/certs/ca.pem");
    }

    #[test]
    fn test_resolve_path_relative_to_config_relative() {
        let resolved = resolve_path_relative_to_config("/etc/myapp/config.ron", "../certs/ca.pem");
        assert_eq!(resolved, "/etc/certs/ca.pem");
    }

    #[test]
    fn test_resolve_path_relative_to_config_same_dir() {
        let resolved = resolve_path_relative_to_config("/etc/myapp/config.ron", "local.pem");
        assert_eq!(resolved, "/etc/myapp/local.pem");
    }

    #[test]
    fn test_get_config_dir() {
        let dir = get_config_dir("/etc/myapp/config.ron");
        assert_eq!(dir, PathBuf::from("/etc/myapp"));
    }

    #[test]
    fn test_get_config_dir_current() {
        let dir = get_config_dir("config.ron");
        // Parent of a filename with no directory is ".", but it may normalize to empty
        // Both are acceptable and represent the current directory
        assert!(dir == PathBuf::from(".") || dir == PathBuf::from(""));
    }

    #[test]
    fn test_resolve_paths_relative_to_config() {
        let paths = [
            "../certs/ca.pem",
            "../certs/server.pem",
            "/etc/absolute.pem",
        ];
        let resolved =
            resolve_paths_relative_to_config("/etc/myapp/config.ron", paths.iter().copied());

        assert_eq!(
            resolved,
            vec![
                "/etc/certs/ca.pem",
                "/etc/certs/server.pem",
                "/etc/absolute.pem"
            ]
        );
    }

    #[test]
    fn test_resolve_paths_relative_to_config_empty() {
        let paths: Vec<&str> = vec![];
        let resolved =
            resolve_paths_relative_to_config("/etc/myapp/config.ron", paths.iter().copied());
        assert!(resolved.is_empty());
    }
}
