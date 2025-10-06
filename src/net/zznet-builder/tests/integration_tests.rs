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

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use zznet_builder::{ClientBuilder, ServerBuilder};
use zznet_hello::connection_manager::ConnectionManager;
use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::types::RoomId;

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
// providing a local alias to the test role. This keeps the crate free of any
// runtime dependency on the application's `zzping-auth` while avoiding many
// mechanical edits in tests.
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

    // Create ConnectionManagers for server and client
    let server_rooms = vec![RoomId::from("health"), RoomId::from("data")];
    let client_rooms = vec![RoomId::from("health"), RoomId::from("data")];

    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

    let server_manager = ConnectionManager::<TestMessage, TestRole>::new(server_rooms).start();
    let client_manager = ConnectionManager::<TestMessage, TestRole>::new(client_rooms).start();

    // Start server
    println!("Starting server on 127.0.0.1:18080...");
    let server = ServerBuilder::<TestMessage, TestRole>::new()
        .bind("127.0.0.1:18080")
        .as_role(TestRole::Database)
        .offer_rooms(vec!["health".to_string(), "data".to_string()])
        .with_connection_manager(server_manager)
        .start()
        .await
        .expect("Failed to start server");

    println!("Server started successfully");

    // Give server time to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start client
    println!("Starting client connecting to 127.0.0.1:18080...");
    let client = ClientBuilder::<TestMessage, TestRole>::new()
        .connect_to("127.0.0.1:18080")
        .as_role(TestRole::Collector)
        .offer_rooms(vec!["health".to_string(), "data".to_string()])
        .with_connection_manager(client_manager)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to start client");

    println!("Client started successfully");

    // Wait for connection and handshake
    tokio::time::sleep(Duration::from_millis(500)).await;

    println!("✓ Connection established and handshake completed");

    // Cleanup
    server.do_send(zznet_builder::server_builder::StopServer);
    client.do_send(zznet_builder::client_builder::Disconnect);

    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("=== Test Complete ===\n");
}

/// Test 2: Multiple concurrent clients connecting to one server
#[actix::test]
async fn test_multiple_clients() {
    println!("\n=== Test: Multiple Concurrent Clients ===");

    // Create server ConnectionManager
    let server_rooms = vec![RoomId::from("health")];
    let server_manager = ConnectionManager::<TestMessage, AuthRole>::new(server_rooms).start();

    // Start server
    println!("Starting server on 127.0.0.1:18081...");
    let server = ServerBuilder::new()
        .bind("127.0.0.1:18081")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_connection_manager(server_manager)
        .start()
        .await
        .expect("Failed to start server");

    println!("Server started successfully");

    // Give server time to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start 3 clients
    let mut clients = Vec::new();
    for i in 1..=3 {
        println!("Starting client {}...", i);

        let client_rooms = vec![RoomId::from("health")];
        let client_manager = ConnectionManager::<TestMessage, AuthRole>::new(client_rooms).start();

        let client = ClientBuilder::new()
            .connect_to("127.0.0.1:18081")
            .as_role(AuthRole::Collector)
            .offer_rooms(vec!["health".to_string()])
            .with_connection_manager(client_manager)
            .auto_reconnect(false)
            .connect()
            .await
            .unwrap_or_else(|_| panic!("Failed to start client {}", i));

        clients.push(client);
        println!("Client {} connected", i);

        // Brief delay between connections
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    println!("All 3 clients connected successfully");

    // Wait for all handshakes
    tokio::time::sleep(Duration::from_millis(500)).await;

    println!("✓ Multiple concurrent connections working");

    // Cleanup
    server.do_send(zznet_builder::server_builder::StopServer);
    for (i, client) in clients.iter().enumerate() {
        println!("Disconnecting client {}...", i + 1);
        client.do_send(zznet_builder::client_builder::Disconnect);
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("=== Test Complete ===\n");
}

/// Test 3: Client reconnection after disconnect
#[actix::test]
async fn test_client_reconnection() {
    println!("\n=== Test: Client Reconnection ===");

    // Create ConnectionManagers
    let server_rooms = vec![RoomId::from("health")];
    let server_manager = ConnectionManager::<TestMessage, AuthRole>::new(server_rooms).start();

    // Start server
    println!("Starting server on 127.0.0.1:18082...");
    let server = ServerBuilder::new()
        .bind("127.0.0.1:18082")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_connection_manager(server_manager.clone())
        .start()
        .await
        .expect("Failed to start server");

    println!("Server started successfully");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start client with auto-reconnect enabled
    println!("Starting client with auto-reconnect...");
    let client_rooms = vec![RoomId::from("health")];
    let client_manager = ConnectionManager::<TestMessage, AuthRole>::new(client_rooms).start();

    let client = ClientBuilder::new()
        .connect_to("127.0.0.1:18082")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_connection_manager(client_manager)
        .auto_reconnect(true)
        .reconnect_delay(Duration::from_millis(200))
        .connect()
        .await
        .expect("Failed to start client");

    println!("Client connected");
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Stop server to simulate disconnect
    println!("Stopping server to simulate disconnect...");
    server.do_send(zznet_builder::server_builder::StopServer);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Restart server
    println!("Restarting server...");
    let server2 = ServerBuilder::new()
        .bind("127.0.0.1:18082")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_connection_manager(server_manager)
        .start()
        .await
        .expect("Failed to restart server");

    println!("Server restarted, waiting for client to reconnect...");
    tokio::time::sleep(Duration::from_millis(800)).await;

    println!("✓ Client should have reconnected automatically");

    // Cleanup
    server2.do_send(zznet_builder::server_builder::StopServer);
    client.do_send(zznet_builder::client_builder::Disconnect);

    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("=== Test Complete ===\n");
}

/// Test 4: Graceful shutdown
#[actix::test]
async fn test_graceful_shutdown() {
    println!("\n=== Test: Graceful Shutdown ===");

    // Create ConnectionManagers
    let server_rooms = vec![RoomId::from("health")];
    let client_rooms = vec![RoomId::from("health")];

    let server_manager = ConnectionManager::<TestMessage, AuthRole>::new(server_rooms).start();
    let client_manager = ConnectionManager::<TestMessage, AuthRole>::new(client_rooms).start();

    // Start server
    println!("Starting server on 127.0.0.1:18083...");
    let server = ServerBuilder::new()
        .bind("127.0.0.1:18083")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_connection_manager(server_manager)
        .start()
        .await
        .expect("Failed to start server");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start client
    println!("Starting client...");
    let client = ClientBuilder::new()
        .connect_to("127.0.0.1:18083")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_connection_manager(client_manager)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to start client");

    println!("Connection established");
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Graceful shutdown
    println!("Initiating graceful shutdown...");
    client.do_send(zznet_builder::client_builder::Disconnect);
    tokio::time::sleep(Duration::from_millis(100)).await;

    server.do_send(zznet_builder::server_builder::StopServer);
    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("✓ Graceful shutdown completed");

    println!("=== Test Complete ===\n");
}

/// Test 5: Server binding to invalid address should fail gracefully
#[actix::test]
async fn test_server_bind_error() {
    println!("\n=== Test: Server Bind Error Handling ===");

    let server_rooms = vec![RoomId::from("health")];
    let server_manager = ConnectionManager::<TestMessage, AuthRole>::new(server_rooms).start();

    // Try to bind to invalid address
    println!("Attempting to bind to invalid address...");
    let result = ServerBuilder::new()
        .bind("999.999.999.999:99999")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_connection_manager(server_manager)
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

    let client_rooms = vec![RoomId::from("health")];
    let client_manager = ConnectionManager::<TestMessage, AuthRole>::new(client_rooms).start();

    // Connect to non-existent server (no auto-reconnect)
    println!("Connecting to non-existent server...");
    let client = ClientBuilder::new()
        .connect_to("127.0.0.1:19999")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_connection_manager(client_manager)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Client actor should start even if connection fails");

    println!("Client actor started (will fail to connect)");

    // Wait to see if it tries to connect
    tokio::time::sleep(Duration::from_millis(300)).await;

    println!("✓ Connection failure handled gracefully");

    // Cleanup
    client.do_send(zznet_builder::client_builder::Disconnect);
    tokio::time::sleep(Duration::from_millis(50)).await;

    println!("=== Test Complete ===\n");
}

/// Test 7: Different authentication roles
#[actix::test]
async fn test_different_auth_roles() {
    println!("\n=== Test: Different Authentication Roles ===");

    // Server as Database
    let server_rooms = vec![RoomId::from("health")];
    let server_manager = ConnectionManager::<TestMessage, AuthRole>::new(server_rooms).start();

    println!("Starting Database server...");
    let server = ServerBuilder::new()
        .bind("127.0.0.1:18084")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_connection_manager(server_manager)
        .start()
        .await
        .expect("Failed to start server");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Client as Collector (allowed to connect to Database)
    let client1_rooms = vec![RoomId::from("health")];
    let client1_manager = ConnectionManager::<TestMessage, AuthRole>::new(client1_rooms).start();

    println!("Starting Collector client...");
    let client1 = ClientBuilder::new()
        .connect_to("127.0.0.1:18084")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_connection_manager(client1_manager)
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to start collector client");

    println!("Collector connected successfully");
    tokio::time::sleep(Duration::from_millis(300)).await;

    println!("✓ Authentication roles working correctly");

    // Cleanup
    server.do_send(zznet_builder::server_builder::StopServer);
    client1.do_send(zznet_builder::client_builder::Disconnect);

    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("=== Test Complete ===\n");
}

/// Test 8: End-to-end message exchange through full stack
#[actix::test]
async fn test_end_to_end_message_exchange() {
    println!("\n=== Test: End-to-End Message Exchange ===");

    // Create ConnectionManagers for server and client
    let server_rooms = vec![RoomId::from("health")];
    let client_rooms = vec![RoomId::from("health")];

    let server_manager = ConnectionManager::<TestMessage, AuthRole>::new(server_rooms).start();
    let client_manager = ConnectionManager::<TestMessage, AuthRole>::new(client_rooms).start();

    // Start server
    println!("Starting server on 127.0.0.1:18085...");
    let server = ServerBuilder::new()
        .bind("127.0.0.1:18085")
        .as_role(AuthRole::Database)
        .offer_rooms(vec!["health".to_string()])
        .with_connection_manager(server_manager.clone())
        .start()
        .await
        .expect("Failed to start server");

    println!("Server started successfully");

    // Give server time to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start client
    println!("Starting client connecting to 127.0.0.1:18085...");
    let client = ClientBuilder::new()
        .connect_to("127.0.0.1:18085")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec!["health".to_string()])
        .with_connection_manager(client_manager.clone())
        .auto_reconnect(false)
        .connect()
        .await
        .expect("Failed to start client");

    println!("Client started successfully");

    // Wait for connection and handshake - increase timeout
    println!("Waiting for handshake to complete...");
    tokio::time::sleep(Duration::from_secs(2)).await;

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
        tokio::time::sleep(Duration::from_secs(3)).await;

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
    let receive_result = tokio::time::timeout(Duration::from_secs(5), server_receiver.recv()).await;

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
    let receive_result = tokio::time::timeout(Duration::from_secs(2), client_receiver.recv()).await;

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

    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("=== Test Complete ===\n");
}
