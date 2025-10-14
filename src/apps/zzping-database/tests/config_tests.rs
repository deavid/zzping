//! Tests for configuration loading and validation.

use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;
use zzping_database::config::*;

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
            ca_cert_path: certs_dir.join("ca.pem").to_str().unwrap().to_string(),
            server_cert_path: certs_dir
                .join("database.pem")
                .to_str()
                .unwrap()
                .to_string(),
            server_key_path: certs_dir
                .join("database.key")
                .to_str()
                .unwrap()
                .to_string(),
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
            ca_cert_path: "{}",
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
