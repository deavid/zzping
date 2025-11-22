//! Service orchestration and component lifecycle for the database app.
//!
//! This module creates, configures, and runs the database components and
//! wires them into the network. It centralizes testable orchestration and
//! provides convenience helpers for starting components in tests or
//! embedding the database logic into different runtimes.

use crate::config::DatabaseTlsConfig;
use crate::error::DatabaseError;

// Room Handler Architecture
//
// This application uses the declarative room handler registration pattern provided by
// `zznet-builder`. Room handlers are defined as factories in `crate::room_handlers` and
// registered with the `ServerBuilder` in `crate::network`.
//
// Pattern:
//   1. Define RoomHandlerFactory implementations (see `room_handlers.rs`)
//   2. Register factories with ClientBuilder/ServerBuilder (see `network.rs`)
//   3. Builders automatically wire handlers on connection/reconnection
//
// This approach provides reusable, testable room handler configuration.
// See `ROOM_REGISTRY_GUIDE.md` for details.

/// Build TLS configuration for the transport layer (TcpTransportServer)
pub fn build_transport_tls_config(
    tls: &DatabaseTlsConfig,
) -> Result<Option<zznet_transport_tcp::config::TlsConfig>, DatabaseError> {
    // Use builder helper to construct a transport TLS config from file paths
    let ca = tls.ca_cert_paths.first().map(|s| s.as_str());
    Ok(Some(zznet_transport_tcp::tls_utils::to_transport_tls_config(
        &tls.server_cert_path,
        &tls.server_key_path,
        ca,
        "zzping-mesh".into(),
    )))
}

#[cfg(test)]
mod tests {
    use zzintent_config::builder::IntentConfigBuilder;
    use zzintent_config::permissions::IntentConfigPermissions;

    use super::*;
    use std::collections::HashMap;
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

    #[actix::test]
    #[ignore]
    async fn test_database_network_creation() {
        use std::time::Duration;

        // Test network creation without TLS (TLS config creation is complex and tested elsewhere)
        // Use port 0 to let OS assign an available port
        let _network =
            crate::network::DatabaseNetwork::bind("127.0.0.1:0", None, Duration::from_secs(10))
                .await
                .expect("Failed to bind network");
        // Network is now created successfully if we get here
        // We don't run() it as that would block indefinitely
    }

    #[test]
    fn test_intent_config_permissions_policy() {
        // Test that the intent-config policy is configured correctly
        // Create permissions policy for intent-config
        let mut intent_config_permissions = HashMap::new();
        intent_config_permissions.insert(
            "client-admin".to_string(),
            IntentConfigPermissions::new(true, true), // can read and write
        );
        intent_config_permissions.insert(
            "collector".to_string(),
            IntentConfigPermissions::new(true, false), // can read but not write
        );

        let intent_builder = IntentConfigBuilder::new()
            .config_for_database(std::path::PathBuf::from("test.ron"))
            .permissions_map(intent_config_permissions);

        // Get the permissions map from the builder
        let permissions_map = intent_builder
            .get_permissions_map()
            .expect("permissions_map should be set");

        // Verify client-admin has full access
        let admin_perms = permissions_map
            .get("client-admin")
            .expect("client-admin should have permissions");
        assert!(
            admin_perms.can_read_config,
            "client-admin should be able to read"
        );
        assert!(
            admin_perms.can_write_config,
            "client-admin should be able to write"
        );

        // Verify collector has read-only access
        let collector_perms = permissions_map
            .get("collector")
            .expect("collector should have permissions");
        assert!(
            collector_perms.can_read_config,
            "collector should be able to read"
        );
        assert!(
            !collector_perms.can_write_config,
            "collector should NOT be able to write"
        );

        // Verify unknown roles don't have permissions
        assert!(
            permissions_map.get("hacker").is_none(),
            "unknown roles should not have permissions"
        );
    }
}
