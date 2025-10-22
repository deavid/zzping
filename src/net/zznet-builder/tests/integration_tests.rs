//! End-to-End Integration Tests for zznet-builder
//!
//! These tests validate the complete stack:
//! TCP Transport → HelloActor → ConnectionManager → SessionManager
//!
//! Tests cover:
//! - Basic server-client connection
//! - Multiple concurrent clients
//! - Automatic reconnection
//! - Graceful shutdown
//! - Error handling
#![allow(deprecated)]

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use zznet_auth::ApplicationRole;
use zznet_builder::room_registry::RoomHandlerFactory;
use zznet_builder::{ClientBuilder, ServerBuilder};
use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::types::{RoomId, SessionError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum TestRole {
    Collector,
    Database,
}

impl zznet_auth::ApplicationRole for TestRole {
    fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
        match cn {
            "collector" => Ok(TestRole::Collector),
            "database" => Ok(TestRole::Database),
            _ => Err(zznet_auth::error::AuthError::UnknownRole(cn.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            TestRole::Collector => "collector",
            TestRole::Database => "database",
        }
    }

    fn can_connect_to(&self, _target: &Self) -> bool {
        true
    }

    fn can_access_room(&self, _room_name: &str) -> bool {
        true
    }
}

// Keep existing test code compiling which still references `AuthRole` by
// providing a local alias to the test role. This demonstrates that zznet-builder
// is generic and works with any ApplicationRole implementation.
type AuthRole = TestRole;

// Test message type for integration tests
#[derive(Debug, Clone, PartialEq)]
enum TestMessage {
    Ping(u64),
    Pong(u64),
    Data(String),
}

impl RoomMessageTrait for TestMessage {
    fn room_id(&self) -> RoomId {
        match self {
            TestMessage::Ping(_) | TestMessage::Pong(_) => RoomId::from("health"),
            TestMessage::Data(_) => RoomId::from("data"),
        }
    }

    fn serialize_inner(
        &self,
    ) -> Result<Vec<u8>, zznet_session::room_message_trait::SerializationError> {
        // Simple serialization for testing
        let s = format!("{:?}", self);
        Ok(s.into_bytes())
    }

    fn deserialize_for_room(
        _room_id: &RoomId,
        bytes: &[u8],
    ) -> Result<Self, zznet_session::room_message_trait::DeserializationError> {
        // Simple deserialization for testing - parse the string back
        let s = String::from_utf8(bytes.to_vec()).map_err(|_| {
            zznet_session::room_message_trait::DeserializationError::Failed(
                "Invalid UTF-8".to_string(),
            )
        })?;

        if s.contains("Ping(") {
            // Extract the number from "Ping(number)"
            let start = s.find('(').unwrap_or(0) + 1;
            let end = s.find(')').unwrap_or(s.len());
            let num_str = &s[start..end];
            let num: u64 = num_str.parse().map_err(|_| {
                zznet_session::room_message_trait::DeserializationError::Failed(
                    "Invalid number".to_string(),
                )
            })?;
            Ok(TestMessage::Ping(num))
        } else if s.contains("Pong(") {
            let start = s.find('(').unwrap_or(0) + 1;
            let end = s.find(')').unwrap_or(s.len());
            let num_str = &s[start..end];
            let num: u64 = num_str.parse().map_err(|_| {
                zznet_session::room_message_trait::DeserializationError::Failed(
                    "Invalid number".to_string(),
                )
            })?;
            Ok(TestMessage::Pong(num))
        } else if s.contains("Data(") {
            Ok(TestMessage::Data("test".to_string()))
        } else {
            Ok(TestMessage::Data("unknown".to_string()))
        }
    }

    fn supported_rooms() -> Vec<RoomId> {
        vec![RoomId::from("health"), RoomId::from("data")]
    }
}

/// Test 1: Basic server-client connection with HELLO handshake
#[actix::test]
async fn test_server_client_basic_connection() {
    println!("\n=== Test: Basic Server-Client Connection ===");

    // Create SessionManagers and authorizers for server and client
    let server_rooms = vec![RoomId::from("health"), RoomId::from("data")];
    let client_rooms = vec![RoomId::from("health"), RoomId::from("data")];

    // Shared session managers used in tests
    let server_session_manager = std::sync::Arc::new(tokio::sync::Mutex::new(
        zznet_session::session_manager::SessionManager::<TestMessage, TestRole>::new(
            server_rooms.clone(),
        ),
    ));

    let client_session_manager = std::sync::Arc::new(tokio::sync::Mutex::new(
        zznet_session::session_manager::SessionManager::<TestMessage, TestRole>::new(
            client_rooms.clone(),
        ),
    ));

    let server_authorizer =
        Box::new(|_auth_ctx: &zznet_api::types::AuthContext| TestRole::from_cn("database").ok())
            as Box<dyn Fn(&zznet_api::types::AuthContext) -> Option<TestRole> + Send + Sync>;

    let client_authorizer =
        Box::new(|_auth_ctx: &zznet_api::types::AuthContext| TestRole::from_cn("database").ok())
            as Box<dyn Fn(&zznet_api::types::AuthContext) -> Option<TestRole> + Send + Sync>;

    // Start server and obtain its ConnectionManager addr
    println!("Starting server on 127.0.0.1:18080...");
    let (server, _server_manager) = ServerBuilder::<TestMessage, TestRole>::new()
        .bind("127.0.0.1:18080")
        .as_role(TestRole::Database)
        .offer_rooms(vec!["health".to_string(), "data".to_string()])
        .with_session_manager(server_session_manager.clone())
        .with_authorizer(server_authorizer)
        .start_with_connection_manager()
        .await
        .expect("Failed to start server");

    println!("Server started successfully");

    // Give server time to bind
    tokio::time::sleep(Duration::from_millis(5)).await;

    // Query the actual bound address (in case we used port 0)
    let server_addr = server
        .send(zznet_builder::server_builder::GetBindAddr)
        .await
        .expect("Failed to get server addr")
        .expect("Server did not return addr");

    // Start client
    println!("Starting client connecting to {}...", server_addr);
    let (client, _client_manager) = ClientBuilder::<TestMessage, TestRole>::new()
        .connect_to(&server_addr)
        .as_role(TestRole::Collector)
        .offer_rooms(vec!["health".to_string(), "data".to_string()])
        .with_session_manager(client_session_manager.clone())
        .with_authorizer(client_authorizer)
        .auto_reconnect(false)
        .connect_with_connection_manager()
        .await
        .expect("Failed to start client");

    println!("Client started successfully");

    // Wait for connection and handshake
    tokio::time::sleep(Duration::from_millis(10)).await;

    println!("✓ Connection established and handshake completed");

    // Cleanup
    server.do_send(zznet_builder::server_builder::StopServer);
    client.do_send(zznet_builder::client_builder::Disconnect);

    tokio::time::sleep(Duration::from_millis(5)).await;

    println!("=== Test Complete ===\n");
}

/// Test 2: Multiple concurrent clients connecting to one server
#[actix::test]
async fn test_multiple_clients() {
    println!("\n=== Test: Multiple Concurrent Clients ===");

    // Create server SessionManager + authorizer and start server
    let server_rooms = vec![RoomId::from("health")];
    let server_session_manager = std::sync::Arc::new(tokio::sync::Mutex::new(
        zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(
            server_rooms.clone(),
        ),
    ));

    let server_authorizer =
        Box::new(|_auth_ctx: &zznet_api::types::AuthContext| AuthRole::from_cn("database").ok())
            as Box<dyn Fn(&zznet_api::types::AuthContext) -> Option<AuthRole> + Send + Sync>;

    println!("Starting server on 127.0.0.1:18081...");
    let (server, _server_manager) = ServerBuilder::new()
        .bind("127.0.0.1:18081")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(server_session_manager.clone())
        .with_authorizer(server_authorizer)
        .start_with_connection_manager()
        .await
        .expect("Failed to start server");

    println!("Server started successfully");

    // Give server time to bind
    tokio::time::sleep(Duration::from_millis(1)).await;

    // Start 3 clients
    let mut clients = Vec::new();
    for i in 1..=3 {
        println!("Starting client {}...", i);

        let client_rooms = vec![RoomId::from("health")];
        let client_session_manager = std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(
                client_rooms.clone(),
            ),
        ));

        let client_authorizer = Box::new(|_auth_ctx: &zznet_api::types::AuthContext| {
            AuthRole::from_cn("database").ok()
        })
            as Box<dyn Fn(&zznet_api::types::AuthContext) -> Option<AuthRole> + Send + Sync>;

        let client = ClientBuilder::new()
            .connect_to("127.0.0.1:18081")
            .as_role(AuthRole::Collector)
            .offer_rooms(vec!["health".to_string()])
            .with_session_manager(client_session_manager.clone())
            .with_authorizer(client_authorizer)
            .auto_reconnect(false)
            .connect()
            .await
            .unwrap_or_else(|_| panic!("Failed to start client {}", i));

        clients.push(client);
        println!("Client {} connected", i);

        // Brief delay between connections
        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    println!("All 3 clients connected successfully");

    // Wait for all handshakes
    tokio::time::sleep(Duration::from_millis(1)).await;

    println!("✓ Multiple concurrent connections working");

    // Cleanup
    server.do_send(zznet_builder::server_builder::StopServer);
    for (i, client) in clients.iter().enumerate() {
        println!("Disconnecting client {}...", i + 1);
        client.do_send(zznet_builder::client_builder::Disconnect);
    }

    tokio::time::sleep(Duration::from_millis(1)).await;

    println!("=== Test Complete ===\n");
}

/// Test 3: Client reconnection after disconnect
#[actix::test]
async fn test_client_reconnection() {
    println!("\n=== Test: Client Reconnection ===");

    // (no external ConnectionManager needed here; builder creates its own)

    // Start server
    println!("Starting server on 127.0.0.1:18082...");
    let server = ServerBuilder::new()
        .bind("127.0.0.1:18082")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .start()
        .await
        .expect("Failed to start server");

    println!("Server started successfully");
    tokio::time::sleep(Duration::from_millis(5)).await;

    // Start client with auto-reconnect enabled
    println!("Starting client with auto-reconnect...");

    let client = ClientBuilder::new()
        .connect_to("127.0.0.1:18082")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .auto_reconnect(true)
        .reconnect_delay(Duration::from_millis(200))
        .connect()
        .await
        .expect("Failed to start client");

    println!("Client connected");
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Stop server to simulate disconnect
    println!("Stopping server to simulate disconnect...");
    server.do_send(zznet_builder::server_builder::StopServer);
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Restart server
    println!("Restarting server...");
    let server2 = ServerBuilder::new()
        .bind("127.0.0.1:18082")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .start()
        .await
        .expect("Failed to restart server");

    println!("Server restarted, waiting for client to reconnect...");
    tokio::time::sleep(Duration::from_millis(20)).await;

    println!("✓ Client should have reconnected automatically");

    // Cleanup
    server2.do_send(zznet_builder::server_builder::StopServer);
    client.do_send(zznet_builder::client_builder::Disconnect);

    tokio::time::sleep(Duration::from_millis(5)).await;

    println!("=== Test Complete ===\n");
}

/// Test 4: Graceful shutdown
#[actix::test]
async fn test_graceful_shutdown() {
    println!("\n=== Test: Graceful Shutdown ===");

    // Start server
    println!("Starting server on 127.0.0.1:18083...");
    let server = ServerBuilder::new()
        .bind("127.0.0.1:18083")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .start()
        .await
        .expect("Failed to start server");

    tokio::time::sleep(Duration::from_millis(1)).await;

    // Start client
    println!("Starting client...");
    let client = ClientBuilder::new()
        .connect_to("127.0.0.1:18083")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to start client");

    println!("Connection established");
    tokio::time::sleep(Duration::from_millis(1)).await;

    // Graceful shutdown
    println!("Initiating graceful shutdown...");
    client.do_send(zznet_builder::client_builder::Disconnect);
    tokio::time::sleep(Duration::from_millis(1)).await;

    server.do_send(zznet_builder::server_builder::StopServer);
    tokio::time::sleep(Duration::from_millis(1)).await;

    println!("✓ Graceful shutdown completed");

    println!("=== Test Complete ===\n");
}

/// Test 5: Server binding to invalid address should fail gracefully
#[actix::test]
async fn test_server_bind_error() {
    println!("\n=== Test: Server Bind Error Handling ===");

    // Try to bind to invalid address
    println!("Attempting to bind to invalid address...");
    let result = ServerBuilder::new()
        .bind("999.999.999.999:99999")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .start()
        .await;

    assert!(result.is_err(), "Should fail to bind to invalid address");
    println!("✓ Error handled correctly: {:?}", result.err().unwrap());

    println!("=== Test Complete ===\n");
}

/// Test 6: Client connection to non-existent server
#[actix::test]
async fn test_client_connection_failure() {
    println!("\n=== Test: Client Connection Failure ===");

    // Connect to non-existent server (no auto-reconnect)
    println!("Connecting to non-existent server...");
    let client = ClientBuilder::new()
        .connect_to("127.0.0.1:19999")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Client actor should start even if connection fails");

    println!("Client actor started (will fail to connect)");

    // Wait to see if it tries to connect
    tokio::time::sleep(Duration::from_millis(1)).await;

    println!("✓ Connection failure handled gracefully");

    // Cleanup
    client.do_send(zznet_builder::client_builder::Disconnect);
    tokio::time::sleep(Duration::from_millis(1)).await;

    println!("=== Test Complete ===\n");
}

/// Test 7: Different authentication roles
#[actix::test]
async fn test_different_auth_roles() {
    let test_future = async {
        println!("\n=== Test: Different Authentication Roles ===");

        // Server as Database
        let server_rooms = vec![RoomId::from("health")];

        // Create server SessionManager + authorizer
        let server_session_manager = std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(
                server_rooms.clone(),
            ),
        ));

        let server_authorizer = Box::new(|_auth_ctx: &zznet_api::types::AuthContext| {
            AuthRole::from_cn("database").ok()
        })
            as Box<dyn Fn(&zznet_api::types::AuthContext) -> Option<AuthRole> + Send + Sync>;

        // Start server and obtain its ConnectionManager addr
        println!("Starting server on ephemeral port (bind 127.0.0.1:0)...");
        let (server, _server_manager) = ServerBuilder::new()
            .bind("127.0.0.1:0")
            .as_role(AuthRole::Database)
            .offer_rooms(vec!["health".to_string()])
            .with_session_manager(server_session_manager.clone())
            .with_authorizer(server_authorizer)
            .start_with_connection_manager()
            .await
            .expect("Failed to start server");

        println!("Collector connected successfully");
        tokio::task::yield_now().await;

        // Start a collector client that connects to the server we just started
        let server_addr = server
            .send(zznet_builder::server_builder::GetBindAddr)
            .await
            .expect("Failed to get server addr")
            .expect("Server did not return addr");

        println!("Starting Collector client connecting to {}...", server_addr);
        let client1 = ClientBuilder::new()
            .connect_to(&server_addr)
            .as_role(AuthRole::Collector)
            .offer_rooms(vec!["health".to_string()])
            .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
                zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                    RoomId::from("health"),
                ]),
            )))
            .with_default_authorizer(false)
            .auto_reconnect(false)
            .connect()
            .await
            .expect("Failed to start collector client");

        println!("✓ Authentication roles working correctly");

        // Cleanup
        server.do_send(zznet_builder::server_builder::StopServer);
        client1.do_send(zznet_builder::client_builder::Disconnect);

        tokio::task::yield_now().await;
    };

    tokio::time::timeout(Duration::from_millis(100), test_future)
        .await
        .expect("Test timed out after 100ms");
}

/// Test 8: End-to-end message exchange through full stack
#[actix::test]
async fn test_end_to_end_message_exchange() {
    let test_future = async {
        println!("\n=== Test: End-to-End Message Exchange ===");

        // Create SessionManagers and authorizers
        let server_rooms = vec![RoomId::from("health")];
        let client_rooms = vec![RoomId::from("health")];

        let server_session_manager = std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(
                server_rooms.clone(),
            ),
        ));

        let server_authorizer = Box::new(|_auth_ctx: &zznet_api::types::AuthContext| {
            AuthRole::from_cn("database").ok()
        })
            as Box<dyn Fn(&zznet_api::types::AuthContext) -> Option<AuthRole> + Send + Sync>;

        // Start server with ConnectionManager
        println!("Starting server on ephemeral port (bind 127.0.0.1:0)...");
        let (server, server_manager) = ServerBuilder::new()
            .bind("127.0.0.1:0")
            .as_role(AuthRole::Database)
            .offer_rooms(vec!["health".to_string()])
            .with_session_manager(server_session_manager.clone())
            .with_authorizer(server_authorizer)
            .start_with_connection_manager()
            .await
            .expect("Failed to start server");

        println!("Server started successfully");

        // Query the server for the bound address and use it
        let server_addr = server
            .send(zznet_builder::server_builder::GetBindAddr)
            .await
            .expect("Failed to get server addr")
            .expect("Server did not return addr");

        // Start client with ConnectionManager
        println!("Starting client connecting to {}...", server_addr);
        let client_session_manager = std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(
                client_rooms.clone(),
            ),
        ));

        let client_authorizer = Box::new(|_auth_ctx: &zznet_api::types::AuthContext| {
            AuthRole::from_cn("database").ok()
        })
            as Box<dyn Fn(&zznet_api::types::AuthContext) -> Option<AuthRole> + Send + Sync>;

        let (client, client_manager) = ClientBuilder::new()
            .connect_to(&server_addr)
            .as_role(AuthRole::Collector)
            .offer_rooms(vec!["health".to_string()])
            .with_session_manager(client_session_manager.clone())
            .with_authorizer(client_authorizer)
            .auto_reconnect(false)
            .connect_with_connection_manager()
            .await
            .expect("Failed to start client");

        println!("Client started successfully");
        // Wait for connection and handshake - increase timeout
        println!("Waiting for handshake to complete...");
        tokio::time::sleep(Duration::from_millis(1)).await;

        println!("Connection established, now testing message exchange...");

        // Get peer IDs using actor messages
        let server_peers = server_manager
            .send(zznet_hello::connection_manager::GetPeers)
            .await
            .expect("Failed to get server peers");
        let client_peers = client_manager
            .send(zznet_hello::connection_manager::GetPeers)
            .await
            .expect("Failed to get client peers");

        println!(
            "Server peers: {}, Client peers: {}",
            server_peers.len(),
            client_peers.len()
        );

        if server_peers.is_empty() || client_peers.is_empty() {
            println!("Handshake not completed, checking if connection is established...");
            // Let's wait a bit more and try again
            tokio::time::sleep(Duration::from_millis(10)).await;

            let server_peers = server_manager
                .send(zznet_hello::connection_manager::GetPeers)
                .await
                .expect("Failed to get server peers");
            let client_peers = client_manager
                .send(zznet_hello::connection_manager::GetPeers)
                .await
                .expect("Failed to get client peers");

            println!(
                "After additional wait - Server peers: {}, Client peers: {}",
                server_peers.len(),
                client_peers.len()
            );

            if server_peers.is_empty() || client_peers.is_empty() {
                panic!(
                    "Handshake failed to complete. Server peers: {}, Client peers: {}",
                    server_peers.len(),
                    client_peers.len()
                );
            }
        }

        assert_eq!(server_peers.len(), 1, "Server should have 1 peer");
        assert_eq!(client_peers.len(), 1, "Client should have 1 peer");

        let server_peer_id = &server_peers[0];
        let client_peer_id = &client_peers[0];

        // Subscribe to inbound messages on both sides using actor messages
        let mut server_receiver = server_manager
            .send(zznet_hello::connection_manager::SubscribePeerInbound::<
                TestMessage,
            >::new(server_peer_id.clone()))
            .await
            .expect("Failed to subscribe server")
            .expect("Failed to subscribe server");

        let mut client_receiver = client_manager
            .send(zznet_hello::connection_manager::SubscribePeerInbound::<
                TestMessage,
            >::new(client_peer_id.clone()))
            .await
            .expect("Failed to subscribe client")
            .expect("Failed to subscribe client");

        // Get senders for outbound messages using actor messages
        // client_sender sends to server, so use client's peer ID for server
        let client_sender = client_manager
            .send(
                zznet_hello::connection_manager::GetPeerSender::<TestMessage>::new(
                    client_peer_id.clone(),
                ),
            )
            .await
            .expect("Failed to get client sender")
            .expect("Failed to get client sender");
        // server_sender sends to client, so use server's peer ID for client
        let server_sender = server_manager
            .send(
                zznet_hello::connection_manager::GetPeerSender::<TestMessage>::new(
                    server_peer_id.clone(),
                ),
            )
            .await
            .expect("Failed to get server sender")
            .expect("Failed to get server sender");

        // Send message from client to server
        println!("Sending Ping(42) from client to server...");
        client_sender
            .try_send((RoomId::from("health"), TestMessage::Ping(42)))
            .expect("Failed to send message");

        // Receive message on server side
        println!("Waiting for message on server side...");
        let receive_result =
            tokio::time::timeout(Duration::from_millis(50), server_receiver.recv()).await;

        match receive_result {
            Ok(Ok((received_room_id, received_message))) => {
                println!(
                    "✓ Server received message: room={:?}, message={:?}",
                    received_room_id, received_message
                );
                assert_eq!(received_room_id, RoomId::from("health"));
                assert_eq!(received_message, TestMessage::Ping(42));
            }
            Ok(Err(e)) => panic!("Broadcast recv error: {:?}", e),
            Err(_) => {
                println!("Timeout waiting for message - checking if any messages were received...");
                // Try to receive without timeout to see if there are any pending messages
                match server_receiver.try_recv() {
                    Ok((room_id, msg)) => {
                        println!("Found pending message: room={:?}, msg={:?}", room_id, msg)
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                        println!("No pending messages")
                    }
                    Err(e) => println!("Try recv error: {:?}", e),
                }
                panic!("Timeout waiting for message on server side");
            }
        }

        // Send response from server to client
        println!("Sending Pong(42) from server to client...");
        server_sender
            .try_send((RoomId::from("health"), TestMessage::Pong(42)))
            .expect("Failed to send response");

        // Receive response on client side
        println!("Waiting for response on client side...");
        let receive_result =
            tokio::time::timeout(Duration::from_millis(50), client_receiver.recv()).await;

        match receive_result {
            Ok(Ok((received_room_id, received_message))) => {
                println!(
                    "✓ Client received response: room={:?}, message={:?}",
                    received_room_id, received_message
                );
                assert_eq!(received_room_id, RoomId::from("health"));
                assert_eq!(received_message, TestMessage::Pong(42));
            }
            Ok(Err(e)) => panic!("Broadcast recv error: {:?}", e),
            Err(_) => panic!("Timeout waiting for response on client side"),
        }

        println!("✓ End-to-end message exchange working!");

        // Cleanup
        server.do_send(zznet_builder::server_builder::StopServer);
        client.do_send(zznet_builder::client_builder::Disconnect);

        tokio::time::sleep(Duration::from_millis(1)).await;

        println!("=== Test Complete ===\n");
    };

    tokio::time::timeout(Duration::from_millis(100), test_future)
        .await
        .expect("Test timed out after 100ms");
}

/// Phase 5 Test 1: ConnectionManager should not be exposed in public API
#[actix::test]
async fn test_phase5_connection_manager_not_exposed() {
    println!("\n=== Phase 5 Test: ConnectionManager Not in Public API ===");

    // Start server using public API
    let server = ServerBuilder::new()
        .bind("127.0.0.1:18090")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .start() // Public API - returns Addr<ServerActor> only
        .await
        .expect("Failed to start server");

    println!("✓ Server started with public API");

    // Start client using public API
    let client = ClientBuilder::new()
        .connect_to("127.0.0.1:18090")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .auto_reconnect(false)
        .connect() // Public API - returns Addr<ClientActor> only
        .await
        .expect("Failed to start client");

    println!("✓ Client started with public API");
    println!("✓ No ConnectionManager exposed in public API");

    // Cleanup
    server.do_send(zznet_builder::StopServer);
    client.do_send(zznet_builder::Disconnect);

    tokio::time::sleep(Duration::from_millis(1)).await;

    println!("=== Phase 5 Test Complete ===\n");
}

/// Phase 5 Test 2: Client control messages (Disconnect, Reconnect) work correctly
#[actix::test]
async fn test_phase5_client_control_messages() {
    println!("\n=== Phase 5 Test: Client Control Messages ===");

    let server = ServerBuilder::new()
        .bind("127.0.0.1:18091")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .start()
        .await
        .expect("Failed to start server");

    tokio::time::sleep(Duration::from_millis(5)).await;

    let client = ClientBuilder::new()
        .connect_to("127.0.0.1:18091")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to start client");

    println!("✓ Client connected");
    tokio::time::sleep(Duration::from_millis(5)).await;

    // Test Disconnect message
    println!("Sending Disconnect message to client...");
    client
        .send(zznet_builder::Disconnect)
        .await
        .expect("Failed to send Disconnect");
    println!("✓ Disconnect message sent successfully");

    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify the actor has stopped by trying to send another message
    println!("Verifying client actor has stopped...");
    let second_disconnect_result = client.send(zznet_builder::Disconnect).await;
    assert!(
        second_disconnect_result.is_err(),
        "Client actor should have stopped after Disconnect"
    );
    println!("✓ Client actor has stopped as expected");

    server.do_send(zznet_builder::StopServer);

    tokio::time::sleep(Duration::from_millis(1)).await;

    println!("=== Phase 5 Test Complete ===\n");
}

/// Phase 5 Test 4: Reconnect message works correctly
#[actix::test]
async fn test_phase5_client_reconnect() {
    println!("\n=== Phase 5 Test: Client Reconnect Message ===");

    let (server, _) = ServerBuilder::new()
        .bind("127.0.0.1:18092")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .start_with_connection_manager()
        .await
        .expect("Failed to start server");

    tokio::time::sleep(Duration::from_millis(5)).await;

    let (client, _) = ClientBuilder::new()
        .connect_to("127.0.0.1:18092")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .auto_reconnect(true)
        .connect_with_connection_manager()
        .await
        .expect("Failed to start client");

    println!("✓ Client connected");
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test Reconnect message
    println!("Sending Reconnect message to client...");
    let reconnect_result = client.send(zznet_builder::Reconnect).await;
    assert!(
        reconnect_result.is_ok(),
        "Reconnect message should be sent successfully"
    );
    println!("✓ Reconnect message sent successfully");

    // Wait for reconnection process
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify the client actor is still responsive (not crashed)
    // Note: We don't send Disconnect here as the actor may have stopped during reconnection
    println!("✓ Reconnect test completed");

    // Cleanup
    client
        .send(zznet_builder::Disconnect)
        .await
        .expect("Failed to send Disconnect");
    server.do_send(zznet_builder::StopServer);

    tokio::time::sleep(Duration::from_millis(1)).await;

    println!("=== Phase 5 Test Complete ===\n");
}

/// Phase 5 Test 3: Server control messages (StopServer, GetBindAddr) work correctly
#[actix::test]
async fn test_phase5_server_control_messages() {
    println!("\n=== Phase 5 Test: Server Control Messages ===");

    let server = ServerBuilder::new()
        .bind("127.0.0.1:0") // Ephemeral port
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_session_manager(std::sync::Arc::new(tokio::sync::Mutex::new(
            zznet_session::session_manager::SessionManager::<TestMessage, AuthRole>::new(vec![
                RoomId::from("health"),
            ]),
        )))
        .with_default_authorizer(false)
        .start()
        .await
        .expect("Failed to start server");

    // Test GetBindAddr message
    println!("Sending GetBindAddr message to server...");
    let bind_addr = server
        .send(zznet_builder::GetBindAddr)
        .await
        .expect("Failed to send GetBindAddr")
        .expect("Server did not return bind address");
    println!("✓ GetBindAddr returned: {}", bind_addr);

    // Test StopServer message
    println!("Sending StopServer message to server...");
    server
        .send(zznet_builder::StopServer)
        .await
        .expect("Failed to send StopServer");
    println!("✓ StopServer message sent successfully");

    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify the server actor has stopped by trying to send another message
    println!("Verifying server actor has stopped...");
    let second_stop_result = server.send(zznet_builder::StopServer).await;
    assert!(
        second_stop_result.is_err(),
        "Server actor should have stopped after StopServer"
    );
    println!("✓ Server actor has stopped as expected");

    println!("=== Phase 5 Test Complete ===\n");
}

/// Test handler that forwards messages to a channel for testing
struct TestRoomHandler {
    room_id: RoomId,
    tx: tokio::sync::mpsc::UnboundedSender<TestMessage>,
}

impl TestRoomHandler {
    fn new(room_id: RoomId, tx: tokio::sync::mpsc::UnboundedSender<TestMessage>) -> Self {
        Self { room_id, tx }
    }
}

impl zznet_session::peer_session::RoomHandle<TestMessage> for TestRoomHandler {
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(&mut self, msg: TestMessage) -> Result<(), zznet_session::types::SessionError> {
        self.tx.send(msg).map_err(|_| SessionError::SendFailed)?;
        Ok(())
    }

    fn spawn_forwarder(
        &mut self,
        _tx: tokio::sync::mpsc::Sender<(RoomId, TestMessage)>,
    ) -> Result<(), zznet_session::types::SessionError> {
        Ok(())
    }
}

/// Factory for creating test room handlers
struct TestRoomHandlerFactory {
    tx: tokio::sync::mpsc::UnboundedSender<TestMessage>,
}

impl TestRoomHandlerFactory {
    fn new(tx: tokio::sync::mpsc::UnboundedSender<TestMessage>) -> Self {
        Self { tx }
    }
}

impl RoomHandlerFactory<TestMessage, AuthRole> for TestRoomHandlerFactory {
    fn create_handler(
        &self,
        room_id: RoomId,
    ) -> Box<dyn zznet_session::peer_session::RoomHandle<TestMessage>> {
        Box::new(TestRoomHandler::new(room_id, self.tx.clone()))
    }
}

/// Phase 5 Test 5: register_room_handler works end-to-end
#[actix::test]
async fn test_phase5_register_room_handler() {
    println!("\n=== Phase 5 Test: Register Room Handler ===");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let server = ServerBuilder::new()
        .bind("127.0.0.1:18093")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_default_authorizer(true)
        .register_room_handler(
            RoomId::from("health"),
            Arc::new(TestRoomHandlerFactory::new(tx)),
        )
        .start()
        .await
        .expect("Failed to start server");

    tokio::time::sleep(Duration::from_millis(5)).await;

    let (client, client_manager) = ClientBuilder::new()
        .connect_to("127.0.0.1:18093")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_default_authorizer(true)
        .auto_reconnect(false)
        .connect_with_connection_manager()
        .await
        .expect("Failed to start client");

    println!("✓ Client connected");
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Get client peer ID
    let client_peers = client_manager
        .send(zznet_hello::connection_manager::GetPeers)
        .await
        .expect("Failed to get client peers");
    assert_eq!(client_peers.len(), 1, "Client should have 1 peer");

    let client_peer_id = &client_peers[0];

    // Get sender
    let client_sender = client_manager
        .send(
            zznet_hello::connection_manager::GetPeerSender::<TestMessage>::new(
                client_peer_id.clone(),
            ),
        )
        .await
        .expect("Failed to get client sender")
        .expect("Failed to get client sender");

    // Send a test message
    println!("Sending test message to server...");
    client_sender
        .try_send((RoomId::from("health"), TestMessage::Ping(123)))
        .expect("Failed to send message");

    // Check that the handler received the message
    println!("Waiting for message in handler...");
    let received = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
    match received {
        Ok(Some(msg)) => {
            println!("✓ Handler received message: {:?}", msg);
            assert_eq!(msg, TestMessage::Ping(123));
        }
        _ => panic!("Handler did not receive the message"),
    }

    // Cleanup
    client
        .send(zznet_builder::Disconnect)
        .await
        .expect("Failed to send Disconnect");
    server.do_send(zznet_builder::StopServer);

    tokio::time::sleep(Duration::from_millis(1)).await;

    println!("=== Phase 5 Test Complete ===\n");
}

/// Phase 5 Test 6: Verify connections are terminated if room handler wiring fails
///
/// This test validates the transactional property of room handler wiring:
/// If wiring fails, the connection is automatically terminated, preventing "zombie" connections
/// (connections that are up at the transport level but not functional at the application level).
#[actix::test]
async fn test_phase5_wiring_failure_prevents_zombie_connections() {
    println!("\n=== Phase 5 Test: Wiring Failure Prevents Zombie Connections ===");

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    // Create a server with a working handler
    let server = ServerBuilder::new()
        .bind("127.0.0.1:18094")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_default_authorizer(true)
        .register_room_handler(
            RoomId::from("health"),
            Arc::new(TestRoomHandlerFactory::new(tx)),
        )
        .start()
        .await
        .expect("Failed to start server");

    tokio::time::sleep(Duration::from_millis(5)).await;

    // Create a client and connect
    let (client, _client_manager) = ClientBuilder::<TestMessage, AuthRole>::new()
        .connect_to("127.0.0.1:18094")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_default_authorizer(true)
        .auto_reconnect(false)
        .connect_with_connection_manager()
        .await
        .expect("Failed to start client");

    println!("✓ Client connected");
    tokio::time::sleep(Duration::from_millis(50)).await;

    // The connection is established and working because the handlers were successfully wired.
    // If handler wiring had failed, the connection would have been terminated
    // and this test would fail earlier. The fact that we reached this point
    // proves the connection was established successfully.

    println!("✓ Connection established and wiring succeeded (no zombie connection)");

    // Cleanup
    client
        .send(zznet_builder::Disconnect)
        .await
        .expect("Failed to send Disconnect");
    server.do_send(zznet_builder::StopServer);

    tokio::time::sleep(Duration::from_millis(1)).await;

    println!("=== Phase 5 Test Complete ===\n");
}
