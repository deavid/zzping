//! Tests for service orchestration.

use std::path::Path;
use zzping_collector::{config::*, CollectorService};

/// Helper to create a valid test config.
fn create_test_config() -> CollectorConfig {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let certs_dir = workspace_root.join("test_certs");
    CollectorConfig {
        collector_id: "test-collector".into(),
        database_host: "127.0.0.1".into(),
        database_port: 8443,
        tls: TlsConfig {
            ca_cert_path: certs_dir.join("ca.pem").to_str().unwrap().to_string(),
            client_cert_path: certs_dir
                .join("collector.pem")
                .to_str()
                .unwrap()
                .to_string(),
            client_key_path: certs_dir
                .join("collector.key")
                .to_str()
                .unwrap()
                .to_string(),
        },
        components: ComponentConfig {
            heartbeat_interval_secs: 5,
            memdb_batch_size: 50,
        },
    }
}

#[test]
fn test_service_creation() {
    let config = create_test_config();
    let service = CollectorService::new(config);
    assert!(service.is_ok());
}

#[test]
fn test_service_creation_validates_config() {
    let mut config = create_test_config();
    config.collector_id = String::new(); // Invalid!

    let result = CollectorService::new(config);
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
        ca_cert_path: certs_dir.join("ca.pem").to_str().unwrap().to_string(),
        client_cert_path: certs_dir
            .join("collector.pem")
            .to_str()
            .unwrap()
            .to_string(),
        client_key_path: certs_dir
            .join("collector.key")
            .to_str()
            .unwrap()
            .to_string(),
    };

    let result = CollectorService::load_tls_config(&tls_config);
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
        ca_cert_path: "nonexistent.pem".into(),
        client_cert_path: certs_dir
            .join("collector.pem")
            .to_str()
            .unwrap()
            .to_string(),
        client_key_path: certs_dir
            .join("collector.key")
            .to_str()
            .unwrap()
            .to_string(),
    };

    let result = CollectorService::load_tls_config(&tls_config);
    assert!(result.is_err(), "Should fail with missing CA");
}
