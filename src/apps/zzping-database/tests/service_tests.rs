//! Tests for service orchestration.

use std::path::Path;
use zzping_database::{config::*, DatabaseService};

/// Helper to create a valid test config.
fn create_test_config() -> DatabaseConfig {
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
fn test_service_creation() {
    let config = create_test_config();
    let service = DatabaseService::new(config);
    assert!(service.is_ok());
}

#[test]
fn test_service_creation_validates_config() {
    let mut config = create_test_config();
    config.bind_host = String::new(); // Invalid!

    let result = DatabaseService::new(config);
    assert!(result.is_err());
}

#[test]
fn test_tls_config_loads_valid_certs() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let certs_dir = workspace_root.join("test_certs");
    let tls_config = TlsConfig {
        ca_cert_paths: vec![certs_dir.join("ca.pem").to_str().unwrap().to_string()],
        server_cert_path: certs_dir.join("database.pem").to_str().unwrap().to_string(),
        server_key_path: certs_dir.join("database.key").to_str().unwrap().to_string(),
    };

    let result = DatabaseService::load_tls_config(&tls_config);
    assert!(result.is_ok(), "TLS config should load successfully");
}

#[test]
fn test_tls_config_fails_missing_ca() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let certs_dir = workspace_root.join("test_certs");
    let tls_config = TlsConfig {
        ca_cert_paths: vec!["nonexistent.pem".into()],
        server_cert_path: certs_dir.join("database.pem").to_str().unwrap().to_string(),
        server_key_path: certs_dir.join("database.key").to_str().unwrap().to_string(),
    };

    let result = DatabaseService::load_tls_config(&tls_config);
    assert!(result.is_err(), "Should fail with missing CA");
}
