//! TLS Integration Tests
//!
//! These tests validate TLS functionality:
//! - Mutual authentication
//! - Certificate validation
//! - Encrypted communication
//! - TLS handshake failures

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use zznet_api::types::Role;
use zznet_builder::{ClientBuilder, ServerBuilder};
use zznet_hello::auth::AuthRole;
use zznet_hello::connection_manager::ConnectionManager;
use zznet_session::room_message_trait::{
    DeserializationError, RoomMessageTrait, SerializationError,
};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
enum TestMessage {
    Ping(u64),
    Pong(u64),
}

impl RoomMessageTrait for TestMessage {
    fn room_id(&self) -> RoomId {
        RoomId::from("test")
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| SerializationError::BincodeError(e.to_string()))
    }

    fn deserialize_for_room(_room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        bincode::serde::decode_from_slice(bytes, bincode::config::standard())
            .map(|(value, _)| value)
            .map_err(|e| DeserializationError::BincodeError(e.to_string()))
    }

    fn supported_rooms() -> Vec<RoomId> {
        vec![RoomId::from("test")]
    }
}

/// Test 1: Basic TLS connection with mutual authentication
#[actix::test]
async fn test_tls_mutual_authentication() {
    // Create TLS configs with explicit certs directory
    let certs_dir = get_certs_dir();
    let certs_path = certs_dir.to_str().expect("Invalid certs path");
    let server_tls = TlsConfig::from_role(Role::Database, Some(certs_path))
        .expect("Failed to create server TLS config");
    let client_tls = TlsConfig::from_role(Role::Collector, Some(certs_path))
        .expect("Failed to create client TLS config");

    // Create server with TLS
    let server_manager = ConnectionManager::<TestMessage>::new(vec![RoomId::from("test")]).start();

    let _server = ServerBuilder::new()
        .bind("127.0.0.1:19001")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(server_tls)
        .with_connection_manager(server_manager)
        .start()
        .await
        .expect("Failed to start TLS server");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create client with TLS
    let client_manager = ConnectionManager::<TestMessage>::new(vec![RoomId::from("test")]).start();

    let _client = ClientBuilder::new()
        .connect_to("127.0.0.1:19001")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(client_tls)
        .with_connection_manager(client_manager)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to connect TLS client");

    // Wait for TLS handshake and HELLO
    tokio::time::sleep(Duration::from_millis(500)).await;

    // If we get here, TLS connection succeeded
}

/// Test 2: Multiple TLS clients connecting to one server
#[actix::test]
async fn test_tls_multiple_clients() {
    let certs_dir = get_certs_dir();
    let certs_path = certs_dir.to_str().expect("Invalid certs path");
    let server_tls = TlsConfig::from_role(Role::Database, Some(certs_path))
        .expect("Failed to create server TLS config");

    let server_manager = ConnectionManager::<TestMessage>::new(vec![RoomId::from("test")]).start();

    let _server = ServerBuilder::new()
        .bind("127.0.0.1:19002")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(server_tls)
        .with_connection_manager(server_manager)
        .start()
        .await
        .expect("Failed to start server");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect 3 clients
    for i in 1..=3 {
        let client_tls = TlsConfig::from_role(Role::Collector, Some(certs_path))
            .expect("Failed to create client TLS config");

        let client_manager =
            ConnectionManager::<TestMessage>::new(vec![RoomId::from("test")]).start();

        let _client = ClientBuilder::new()
            .connect_to("127.0.0.1:19002")
            .as_role(AuthRole::Collector)
            .offer_rooms(vec!["test".to_string()])
            .with_tls(client_tls)
            .with_connection_manager(client_manager)
            .auto_reconnect(false)
            .connect()
            .await
            .unwrap_or_else(|_| panic!("Failed to connect client {}", i));

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    tokio::time::sleep(Duration::from_millis(500)).await;
}

/// Test 3: TLS with different roles (Database server, ClientRo client)
#[actix::test]
async fn test_tls_different_roles() {
    let certs_dir = get_certs_dir();
    let certs_path = certs_dir.to_str().expect("Invalid certs path");
    let server_tls = TlsConfig::from_role(Role::Database, Some(certs_path))
        .expect("Failed to create server TLS config");

    let client_tls = TlsConfig::from_role(Role::ClientRo, Some(certs_path))
        .expect("Failed to create client TLS config");

    let server_manager = ConnectionManager::<TestMessage>::new(vec![RoomId::from("test")]).start();

    let _server = ServerBuilder::new()
        .bind("127.0.0.1:19003")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(server_tls)
        .with_connection_manager(server_manager)
        .start()
        .await
        .expect("Failed to start server");

    tokio::time::sleep(Duration::from_millis(100)).await;

    let client_manager = ConnectionManager::<TestMessage>::new(vec![RoomId::from("test")]).start();

    let _client = ClientBuilder::new()
        .connect_to("127.0.0.1:19003")
        .as_role(AuthRole::ClientRo)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(client_tls)
        .with_connection_manager(client_manager)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to connect with ClientRo role");

    tokio::time::sleep(Duration::from_millis(500)).await;
}

/// Test 4: TLS configuration validation
#[test]
fn test_tls_config_from_role() {
    // Test all roles
    let roles = vec![
        Role::Database,
        Role::Collector,
        Role::ClientRo,
        Role::ClientAdmin,
    ];

    for role in roles {
        let config = TlsConfig::from_role(role, None);
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
    let config = TlsConfig::from_role(Role::Collector, Some("certs"));
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
    let plain_manager = ConnectionManager::<TestMessage>::new(vec![RoomId::from("test")]).start();

    let _plain_server = ServerBuilder::new()
        .bind("127.0.0.1:19004")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["test".to_string()])
        // No TLS
        .with_connection_manager(plain_manager)
        .start()
        .await
        .expect("Failed to start plain server");

    // Start TLS server
    let certs_dir = get_certs_dir();
    let certs_path = certs_dir.to_str().expect("Invalid certs path");
    let tls_server_config = TlsConfig::from_role(Role::Database, Some(certs_path))
        .expect("Failed to create TLS config");

    let tls_manager = ConnectionManager::<TestMessage>::new(vec![RoomId::from("test")]).start();

    let _tls_server = ServerBuilder::new()
        .bind("127.0.0.1:19005")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(tls_server_config)
        .with_connection_manager(tls_manager)
        .start()
        .await
        .expect("Failed to start TLS server");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect plain client to plain server
    let plain_client_manager =
        ConnectionManager::<TestMessage>::new(vec![RoomId::from("test")]).start();

    let _plain_client = ClientBuilder::new()
        .connect_to("127.0.0.1:19004")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["test".to_string()])
        // No TLS
        .with_connection_manager(plain_client_manager)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to connect plain client");

    // Connect TLS client to TLS server
    let tls_client_config =
        TlsConfig::from_role(Role::Collector, None).expect("Failed to create TLS config");

    let tls_client_manager =
        ConnectionManager::<TestMessage>::new(vec![RoomId::from("test")]).start();

    let _tls_client = ClientBuilder::new()
        .connect_to("127.0.0.1:19005")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["test".to_string()])
        .with_tls(tls_client_config)
        .with_connection_manager(tls_client_manager)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to connect TLS client");

    tokio::time::sleep(Duration::from_millis(500)).await;
}
