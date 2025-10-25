//! TLS Integration Tests
//!
//! These tests validate TLS functionality:
//! - Mutual authentication
//! - Certificate validation
//! - Encrypted communication
//! - TLS handshake failures
#![allow(deprecated)]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use zznet_auth::ApplicationRole;
use zznet_builder::{ClientBuilder, ServerBuilder};
use zznet_session::session_manager::SessionManager;
use zznet_session::types::RoomId;
use zznet_transport_tcp::config::TlsConfig;

/// Get the workspace root directory for accessing test certificates
fn get_workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points to the crate root (zznet-builder)
    // We need to go up to the workspace root
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent() // components/
        .unwrap()
        .parent() // src/
        .unwrap()
        .parent() // workspace root
        .unwrap()
        .to_path_buf()
}

/// Get the certs directory path
fn get_certs_dir() -> PathBuf {
    get_workspace_root().join("test_certs")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum TestRole {
    Collector,
    Database,
    ClientRo,
    ClientAdmin,
}

impl zznet_auth::ApplicationRole for TestRole {
    fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
        match cn {
            "collector" => Ok(TestRole::Collector),
            "database" => Ok(TestRole::Database),
            "clientro" => Ok(TestRole::ClientRo),
            "clientadmin" => Ok(TestRole::ClientAdmin),
            _ => Err(zznet_auth::error::AuthError::UnknownRole(cn.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            TestRole::Collector => "collector",
            TestRole::Database => "database",
            TestRole::ClientRo => "clientro",
            TestRole::ClientAdmin => "clientadmin",
        }
    }

    fn can_connect_to(&self, _target: &Self) -> bool {
        true
    }

    fn can_access_room(&self, _room_name: &str) -> bool {
        true
    }
}

/// Helper: Create a SessionManager and an authorizer used by builders in tests
type TestSessionManager = Arc<Mutex<SessionManager<TestRole>>>;

type TestAuthorizer = Box<dyn Fn(&zznet_api::types::AuthContext) -> Option<TestRole> + Send + Sync>;

fn create_test_session_manager() -> (TestSessionManager, TestAuthorizer) {
    let offered_rooms = vec![RoomId::from("test")];
    let sm = SessionManager::<TestRole>::new(offered_rooms);
    let session_manager = Arc::new(Mutex::new(sm));
    let authorizer = Box::new(|_auth_ctx: &zznet_api::types::AuthContext| Some(TestRole::Database))
        as Box<dyn Fn(&zznet_api::types::AuthContext) -> Option<TestRole> + Send + Sync>;
    (session_manager, authorizer)
}

/// Test 1: Basic TLS connection with mutual authentication
#[actix::test]
async fn test_tls_mutual_authentication() {
    // Create TLS configs with explicit certs directory
    let certs_dir = get_certs_dir();
    let certs_path = certs_dir.to_str().expect("Invalid certs path");
    let server_tls = TlsConfig::from_role_name(TestRole::Database.as_str(), Some(certs_path))
        .expect("Failed to create server TLS config");
    let client_tls = TlsConfig::from_role_name(TestRole::Collector.as_str(), Some(certs_path))
        .expect("Failed to create client TLS config");

    // Create server with TLS
    let (server_session_manager, server_authorizer) = create_test_session_manager();

    let _server = ServerBuilder::<TestRole>::new()
        .bind("127.0.0.1:19001")
        .as_role(TestRole::Database)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(server_tls)
        .with_session_manager(server_session_manager.clone())
        .with_authorizer(server_authorizer)
        .start()
        .await
        .expect("Failed to start TLS server");

    tokio::time::sleep(Duration::from_millis(5)).await;

    // Create client with TLS
    let (client_session_manager, client_authorizer) = create_test_session_manager();

    let _client = ClientBuilder::<TestRole>::new()
        .connect_to("127.0.0.1:19001")
        .as_role(TestRole::Collector)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(client_tls)
        .with_session_manager(client_session_manager.clone())
        .with_authorizer(client_authorizer)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to connect TLS client");

    // Wait for TLS handshake and HELLO
    tokio::time::sleep(Duration::from_millis(5)).await;

    // If we get here, TLS connection succeeded
}

/// Test 2: Multiple TLS clients connecting to one server
#[actix::test]
async fn test_tls_multiple_clients() {
    let certs_dir = get_certs_dir();
    let certs_path = certs_dir.to_str().expect("Invalid certs path");
    let server_tls = TlsConfig::from_role_name(TestRole::Database.as_str(), Some(certs_path))
        .expect("Failed to create server TLS config");

    let (server_session_manager, server_authorizer) = create_test_session_manager();

    let _server = ServerBuilder::<TestRole>::new()
        .bind("127.0.0.1:19002")
        .as_role(TestRole::Database)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(server_tls)
        .with_session_manager(server_session_manager.clone())
        .with_authorizer(server_authorizer)
        .start()
        .await
        .expect("Failed to start server");

    tokio::time::sleep(Duration::from_millis(5)).await;

    // Connect 3 clients
    for i in 1..=3 {
        let client_tls = TlsConfig::from_role_name(TestRole::Collector.as_str(), Some(certs_path))
            .expect("Failed to create client TLS config");

        let (client_session_manager, client_authorizer) = create_test_session_manager();

        let _client = ClientBuilder::<TestRole>::new()
            .connect_to("127.0.0.1:19002")
            .as_role(TestRole::Collector)
            .offer_rooms(vec!["test".to_string()])
            .with_tls(client_tls)
            .with_session_manager(client_session_manager.clone())
            .with_authorizer(client_authorizer)
            .auto_reconnect(false)
            .connect()
            .await
            .unwrap_or_else(|_| panic!("Failed to connect client {}", i));

        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    tokio::time::sleep(Duration::from_millis(5)).await;
}

/// Test 3: TLS with different roles (Database server, ClientRo client)
#[actix::test]
async fn test_tls_different_roles() {
    let certs_dir = get_certs_dir();
    let certs_path = certs_dir.to_str().expect("Invalid certs path");
    let server_tls = TlsConfig::from_role_name(TestRole::Database.as_str(), Some(certs_path))
        .expect("Failed to create server TLS config");

    let client_tls = TlsConfig::from_role_name(TestRole::ClientRo.as_str(), Some(certs_path))
        .expect("Failed to create client TLS config");

    let (server_session_manager, server_authorizer) = create_test_session_manager();

    let _server = ServerBuilder::<TestRole>::new()
        .bind("127.0.0.1:19003")
        .as_role(TestRole::Database)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(server_tls)
        .with_session_manager(server_session_manager.clone())
        .with_authorizer(server_authorizer)
        .start()
        .await
        .expect("Failed to start server");

    tokio::time::sleep(Duration::from_millis(10)).await;

    let (client_session_manager, client_authorizer) = create_test_session_manager();

    let _client = ClientBuilder::<TestRole>::new()
        .connect_to("127.0.0.1:19003")
        .as_role(TestRole::ClientRo)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(client_tls)
        .with_session_manager(client_session_manager.clone())
        .with_authorizer(client_authorizer)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to connect with ClientRo role");

    tokio::time::sleep(Duration::from_millis(20)).await;
}

/// Test 4: TLS configuration validation
#[test]
fn test_tls_config_from_role() {
    // Test all roles
    let roles = vec![
        TestRole::Database,
        TestRole::Collector,
        TestRole::ClientRo,
        TestRole::ClientAdmin,
    ];

    for role in roles {
        let config = TlsConfig::from_role_name(role.as_str(), None);
        assert!(
            config.is_ok(),
            "Failed to create TLS config for role {:?}",
            role
        );
    }
}

/// Test 5: TLS config with custom certs directory
#[test]
fn test_tls_config_custom_dir() {
    let config = TlsConfig::from_role_name(TestRole::Collector.as_str(), Some("certs"));
    assert!(config.is_ok());

    let config = config.unwrap();
    assert!(
        config
            .cert
            .pem_path
            .to_str()
            .unwrap()
            .contains("certs/collector.pem")
    );
    assert!(
        config
            .cert
            .key_path
            .to_str()
            .unwrap()
            .contains("certs/collector.key")
    );
}

/// Test 6: Plain TCP and TLS can coexist (different ports)
#[actix::test]
async fn test_plain_and_tls_coexist() {
    // Start plain TCP server
    let (plain_session_manager, plain_authorizer) = create_test_session_manager();

    let _plain_server = ServerBuilder::<TestRole>::new()
        .bind("127.0.0.1:19004")
        .as_role(TestRole::Database)
        .offer_rooms(vec!["test".to_string()])
        // No TLS
        .with_session_manager(plain_session_manager.clone())
        .with_authorizer(plain_authorizer)
        .start()
        .await
        .expect("Failed to start plain server");

    // Start TLS server
    let certs_dir = get_certs_dir();
    let certs_path = certs_dir.to_str().expect("Invalid certs path");
    let tls_server_config =
        TlsConfig::from_role_name(TestRole::Database.as_str(), Some(certs_path))
            .expect("Failed to create TLS config");

    let (tls_session_manager, tls_authorizer) = create_test_session_manager();

    let _tls_server = ServerBuilder::<TestRole>::new()
        .bind("127.0.0.1:19005")
        .as_role(TestRole::Database)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(tls_server_config)
        .with_session_manager(tls_session_manager.clone())
        .with_authorizer(tls_authorizer)
        .start()
        .await
        .expect("Failed to start TLS server");

    tokio::time::sleep(Duration::from_millis(10)).await;

    // Connect plain client to plain server
    let (plain_client_session_manager, plain_client_authorizer) = create_test_session_manager();

    let _plain_client = ClientBuilder::<TestRole>::new()
        .connect_to("127.0.0.1:19004")
        .as_role(TestRole::Collector)
        .offer_rooms(vec!["test".to_string()])
        // No TLS
        .with_session_manager(plain_client_session_manager.clone())
        .with_authorizer(plain_client_authorizer)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to connect plain client");

    // Connect TLS client to TLS server
    let tls_client_config = TlsConfig::from_role_name(TestRole::Collector.as_str(), None)
        .expect("Failed to create TLS config");

    let (tls_client_session_manager, tls_client_authorizer) = create_test_session_manager();

    let _tls_client = ClientBuilder::<TestRole>::new()
        .connect_to("127.0.0.1:19005")
        .as_role(TestRole::Collector)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(tls_client_config)
        .with_session_manager(tls_client_session_manager.clone())
        .with_authorizer(tls_client_authorizer)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to connect TLS client");

    tokio::time::sleep(Duration::from_millis(20)).await;
}
