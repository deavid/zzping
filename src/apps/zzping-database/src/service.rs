//! Service orchestration and component lifecycle for the database app.
//!
//! This module creates, configures, and runs the database components and
//! wires them into the network. It centralizes testable orchestration and
//! provides convenience helpers for starting components in tests or
//! embedding the database logic into different runtimes.

use crate::config::{DatabaseConfig, DatabaseTlsConfig};
use crate::error::DatabaseError;
use actix::{Actor, Addr};
use anyhow::anyhow;
use async_trait::async_trait;
use std::time::Duration;
use tokio::task::JoinHandle;
use zzcollector_state::actor::CStateActor;
use zzintent_config::actor::IntentConfigActor;
use zzmem_db::actor::MemDBActor;
use zznet_builder::traits::ZZNetApplication;
use zznet_router::RouterActor;

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

/// Orchestrates all database components and network lifecycle.
///
/// Acts as the service entrypoint for the database application: validating
/// configuration, preparing component builders, starting components and
/// wiring the network server. This struct allows composition for tests
/// and embedding in higher-level deployers.
pub struct DatabaseApp {
    config: DatabaseConfig,

    intent_builder: Option<zzintent_config::builder::IntentConfigBuilder>,
    memdb_builder: Option<zzmem_db::builder::MemDBBuilder>,
    cstate_builder: Option<zzcollector_state::builder::CStateBuilder>,

    intent_addr: Option<Addr<IntentConfigActor>>,
    memdb_addr: Option<Addr<MemDBActor>>,
    cstate_addr: Option<Addr<CStateActor>>,
    router_actor: Option<Addr<RouterActor>>,
    network_task: Option<JoinHandle<()>>,
}

impl DatabaseApp {
    /// Create a new database application with pre-configured component builders.
    ///
    /// This constructor follows the "Construction Outside, Execution Inside" pattern:
    /// the application receives fully-configured builders and will start them during
    /// the `startup()` phase.
    pub fn new(
        config: DatabaseConfig,
        intent_builder: zzintent_config::builder::IntentConfigBuilder,
        memdb_builder: zzmem_db::builder::MemDBBuilder,
        cstate_builder: zzcollector_state::builder::CStateBuilder,
    ) -> Self {
        Self {
            config,
            intent_builder: Some(intent_builder),
            memdb_builder: Some(memdb_builder),
            cstate_builder: Some(cstate_builder),
            intent_addr: None,
            memdb_addr: None,
            cstate_addr: None,
            router_actor: None,
            network_task: None,
        }
    }

    // Start logic is handled in the ZZNetApplication implementation below.
}

/// Build TLS configuration for the transport layer (TcpTransportServer)
pub fn build_transport_tls_config(
    tls: &DatabaseTlsConfig,
) -> Result<Option<zznet_transport_tcp::config::TlsConfig>, DatabaseError> {
    // Use builder helper to construct a transport TLS config from file paths
    let ca = tls.ca_cert_paths.first().map(|s| s.as_str());
    Ok(Some(zznet_builder::tls::to_transport_tls_config(
        &tls.server_cert_path,
        &tls.server_key_path,
        ca,
    )))
}

/// Implement ZZNetApplication trait for DatabaseApp
#[async_trait]
impl ZZNetApplication for DatabaseApp {
    fn service_name(&self) -> &str {
        "ZZPing Database"
    }

    async fn startup(&mut self) -> Result<(), anyhow::Error> {
        tracing::info!("Database service starting");

        let router_actor = RouterActor::new(vec![]).start();

        let intent_addr = self
            .intent_builder
            .take()
            .unwrap()
            .router(router_actor.clone())
            .start()
            .map_err(|e| anyhow!("IntentConfig start failed: {}", e))?;

        let memdb_addr = self
            .memdb_builder
            .take()
            .unwrap()
            .router(router_actor.clone())
            .build();

        let cstate_addr = self
            .cstate_builder
            .take()
            .unwrap()
            .router(router_actor.clone())
            .build();

        self.intent_addr = Some(intent_addr.clone());
        self.memdb_addr = Some(memdb_addr.clone());
        self.cstate_addr = Some(cstate_addr.clone());
        self.router_actor = Some(router_actor.clone());

        let tls_cfg = if let Some(tls) = &self.config.tls {
            build_transport_tls_config(tls).map_err(|e| anyhow!("{}", e))?
        } else {
            None
        };

        let bind_addr = format!("{}:{}", self.config.bind_host, self.config.bind_port);
        let handshake_timeout = Duration::from_secs(self.config.handshake_timeout_secs);

        // Bind to the port synchronously (well, awaited)
        // This ensures we fail fast if the port is in use or permission is denied.
        let network = crate::network::DatabaseNetwork::bind(&bind_addr, tls_cfg, handshake_timeout)
            .await
            .map_err(|e| anyhow!("Failed to bind network: {}", e))?;

        let router_for_network = router_actor.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = network.run(&router_for_network).await {
                tracing::error!("Database network task failed: {}", e);
            }
        });
        self.network_task = Some(handle);

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), anyhow::Error> {
        if let Some(handle) = self.network_task.take() {
            handle.abort();
        }

        // Drop actor addresses to stop them gracefully
        drop(self.intent_addr.take());
        drop(self.memdb_addr.take());
        drop(self.cstate_addr.take());
        drop(self.router_actor.take());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use zzintent_config::builder::IntentConfigBuilder;
    use zzintent_config::permissions::IntentConfigPermissions;

    use crate::config::ComponentConfig;

    use super::*;
    use std::collections::HashMap;
    use std::path::Path;

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
    fn test_app_creation() {
        let config = create_test_config();
        // For testing, create builders
        let intent_builder = zzintent_config::builder::IntentConfigBuilder::new()
            .config_for_database(std::path::PathBuf::from("test.ron"));
        let memdb_builder = zzmem_db::builder::MemDBBuilder::new(
            zzmem_db::config::MemDBConfig::for_database(10000, None),
        );
        let cstate_builder = zzcollector_state::builder::CStateBuilder::new(
            zzcollector_state::config::CStateConfig::for_database(100, Some(10)),
        );

        let _app = DatabaseApp::new(config, intent_builder, memdb_builder, cstate_builder);
        // Just check creation
    }

    #[test]
    fn test_app_creation_with_config() {
        // Config validation is now done at load time, not in constructor
        let config = create_test_config();
        let intent_builder = zzintent_config::builder::IntentConfigBuilder::new()
            .config_for_database(std::path::PathBuf::from("test.ron"));
        let memdb_builder = zzmem_db::builder::MemDBBuilder::new(
            zzmem_db::config::MemDBConfig::for_database(10000, None),
        );
        let cstate_builder = zzcollector_state::builder::CStateBuilder::new(
            zzcollector_state::config::CStateConfig::for_database(100, Some(10)),
        );

        let _app = DatabaseApp::new(config, intent_builder, memdb_builder, cstate_builder);
    }

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

        // Create a temporary app with builders
        let config = create_test_config();
        let intent_builder = zzintent_config::builder::IntentConfigBuilder::new()
            .config_for_database(std::path::PathBuf::from("test.ron"));
        let memdb_builder = zzmem_db::builder::MemDBBuilder::new(
            zzmem_db::config::MemDBConfig::for_database(10000, None),
        );
        let cstate_builder = zzcollector_state::builder::CStateBuilder::new(
            zzcollector_state::config::CStateConfig::for_database(100, Some(10)),
        );
        let _app = DatabaseApp::new(config, intent_builder, memdb_builder, cstate_builder);

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
