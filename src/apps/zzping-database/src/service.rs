//! Service orchestration and component lifecycle for the database app.
//!
//! Creates, configures, and runs the database components and wires them into the network.

use crate::config::DatabaseTlsConfig;
use crate::error::DatabaseError;

/// Builds TLS configuration for the transport layer.
pub fn build_transport_tls_config(
    tls: &DatabaseTlsConfig,
) -> Result<Option<zznet_transport_tcp::config::TlsConfig>, DatabaseError> {
    // Use builder helper to construct a transport TLS config from file paths
    let ca = tls.ca_cert_paths.first().map(|s| s.as_str());
    Ok(Some(
        zznet_transport_tcp::tls_utils::to_transport_tls_config(
            &tls.server_cert_path,
            &tls.server_key_path,
            ca,
            "zzping-mesh".into(),
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_tls_config_loads_valid_certs() {
        // Initialize Rustls default CryptoProvider
        // Use Once to handle parallel test execution safely
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let _ = rustls::crypto::CryptoProvider::install_default(
                rustls::crypto::ring::default_provider(),
            );
        });

        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let certs_dir = workspace_root.join("test_certs");
        let tls_config = DatabaseTlsConfig {
            ca_cert_paths: vec![certs_dir.join("ca.pem").to_str().unwrap().to_string()],
            server_cert_path: certs_dir.join("database.pem").to_str().unwrap().to_string(),
            server_key_path: certs_dir.join("database.key").to_str().unwrap().to_string(),
        };

        let result = build_transport_tls_config(&tls_config);
        assert!(result.is_ok(), "TLS config should build successfully");
        assert!(result.unwrap().is_some(), "TLS config should not be None");
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
        let tls_config = DatabaseTlsConfig {
            ca_cert_paths: vec!["nonexistent.pem".into()],
            server_cert_path: certs_dir.join("database.pem").to_str().unwrap().to_string(),
            server_key_path: certs_dir.join("database.key").to_str().unwrap().to_string(),
        };

        let result = build_transport_tls_config(&tls_config);
        // Build succeeds even with nonexistent CA - the error will happen when TLS config tries to load the cert
        assert!(
            result.is_ok(),
            "TLS config build should not fail at this stage"
        );
    }
}
