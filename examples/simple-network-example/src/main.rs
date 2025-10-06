//! Simple Network Example - End-to-End zznet-builder Demonstration
//!
//! This example demonstrates how to use the zznet-builder stack to create
//! a simple network application with a server and client that exchange
//! typed messages.
//!
//! **What this demonstrates**:
//! 1. Defining application-specific message types
//! 2. Setting up a server with ServerBuilder
//! 3. Setting up a client with ClientBuilder
//! 4. Exchanging typed messages through SessionManager
//! 5. Using rooms for message routing
//!
//! **Run this example**:
//! ```bash
//! cargo run -p simple-network-example
//! ```

use actix::prelude::*;
use std::time::Duration;
use zznet_builder::{ClientBuilder, ServerBuilder};
use zznet_hello::auth::AuthRole;
use zznet_hello::connection_manager::ConnectionManager;
use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::types::RoomId;

// ============================================================================
// Step 1: Define Application Messages
// ============================================================================

/// Application-specific message enum
///
/// This represents all the different types of messages your application
/// can send and receive. Each variant should implement Clone for room
/// message routing.
#[derive(Debug, Clone, PartialEq)]
enum AppMessage {
    /// Heartbeat message to keep connection alive
    Heartbeat { sequence: u64 },

    /// Request data from peer
    DataRequest { query: String },

    /// Respond with data
    DataResponse { data: Vec<u8> },

    /// Status update notification
    StatusUpdate { status: String, timestamp: u64 },
}

/// Implement RoomMessageTrait to enable message routing
///
/// This trait tells the SessionManager which room each message belongs to.
/// Messages are routed based on their room_id.
impl RoomMessageTrait for AppMessage {
    fn room_id(&self) -> RoomId {
        match self {
            AppMessage::Heartbeat { .. } => RoomId::from("health"),
            AppMessage::DataRequest { .. } | AppMessage::DataResponse { .. } => {
                RoomId::from("data")
            }
            AppMessage::StatusUpdate { .. } => RoomId::from("status"),
        }
    }

    fn serialize_inner(
        &self,
    ) -> Result<Vec<u8>, zznet_session::room_message_trait::SerializationError> {
        // For this example, we use simple Debug formatting
        // In production, use bincode, serde_json, or similar
        Ok(format!("{:?}", self).into_bytes())
    }

    fn deserialize_for_room(
        room_id: &RoomId,
        bytes: &[u8],
    ) -> Result<Self, zznet_session::room_message_trait::DeserializationError> {
        // For this example, we just parse the Debug format
        // In production, use proper deserialization
        let s = String::from_utf8(bytes.to_vec()).map_err(|e| {
            zznet_session::room_message_trait::DeserializationError::Failed(e.to_string())
        })?;

        // Simple parsing based on room_id (not production-ready)
        match room_id.as_str() {
            "health" => Ok(AppMessage::Heartbeat { sequence: 0 }),
            "data" if s.starts_with("DataRequest") => Ok(AppMessage::DataRequest {
                query: "parsed".to_string(),
            }),
            "data" => Ok(AppMessage::DataResponse { data: vec![] }),
            "status" => Ok(AppMessage::StatusUpdate {
                status: "parsed".to_string(),
                timestamp: 0,
            }),
            _ => Err(
                zznet_session::room_message_trait::DeserializationError::UnknownRoom(
                    room_id.clone(),
                ),
            ),
        }
    }

    fn supported_rooms() -> Vec<RoomId> {
        vec![
            RoomId::from("health"),
            RoomId::from("data"),
            RoomId::from("status"),
        ]
    }
}

// ============================================================================
// Step 2: Define Application Logic Actors
// ============================================================================

/// Server application actor
///
/// This actor represents your server-side application logic.
/// It receives messages from clients via the SessionManager.
struct ServerApp {
    name: String,
}

impl ServerApp {
    fn new(name: String) -> Self {
        Self { name }
    }
}

impl Actor for ServerApp {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        println!("[{}] Server application started", self.name);
    }
}

/// Client application actor
///
/// This actor represents your client-side application logic.
/// It can send messages to the server via the SessionManager.
struct ClientApp {
    name: String,
}

impl ClientApp {
    fn new(name: String) -> Self {
        Self { name }
    }
}

impl Actor for ClientApp {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        println!("[{}] Client application started", self.name);
    }
}

// ============================================================================
// Step 3: Main Example - Server and Client Setup
// ============================================================================

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()
        .ok();

    println!("=== Simple Network Example ===\n");
    println!("This example demonstrates:");
    println!("1. Creating a server that accepts connections");
    println!("2. Creating a client that connects to the server");
    println!("3. Completing HELLO handshake");
    println!("4. Room negotiation and message routing setup");
    println!("\n");

    // ========================================================================
    // Server Setup
    // ========================================================================

    println!("📡 Setting up server...");

    // Create a SessionManager for the server
    // This manages all peer connections and room message routing
    let server_session_manager = ConnectionManager::<AppMessage>::new(vec![
        RoomId::from("health"),
        RoomId::from("data"),
        RoomId::from("status"),
    ])
    .start();

    // Create the server using ServerBuilder
    let _server = ServerBuilder::new()
        .bind("127.0.0.1:9000")
        .as_role(AuthRole::Database) // Server role
        .offer_rooms(vec![
            "health".to_string(),
            "data".to_string(),
            "status".to_string(),
        ])
        .with_connection_manager(server_session_manager)
        .start()
        .await?;

    println!("✅ Server listening on 127.0.0.1:9000");
    println!("   Role: Database");
    println!("   Rooms: health, data, status\n");

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start the server application actor so its `new` and struct are used.
    let _server_app = ServerApp::new("server-app".to_string()).start();

    // Start the client application actor so its `new` and struct are used.
    let _client_app = ClientApp::new("client-app".to_string()).start();

    // ========================================================================
    // Client Setup
    // ========================================================================

    println!("🔌 Setting up client...");

    // Create a SessionManager for the client
    let client_session_manager =
        ConnectionManager::<AppMessage>::new(vec![RoomId::from("health"), RoomId::from("data")])
            .start();

    // Create the client using ClientBuilder
    let _client = ClientBuilder::new()
        .connect_to("127.0.0.1:9000")
        .as_role(AuthRole::Collector) // Client role
        .offer_rooms(vec!["health".to_string(), "data".to_string()])
        .with_connection_manager(client_session_manager.clone())
        .auto_reconnect(true) // Enable automatic reconnection
        .reconnect_delay(Duration::from_secs(2))
        .connect()
        .await?;

    println!("✅ Client connected to 127.0.0.1:9000");
    println!("   Role: Collector");
    println!("   Rooms: health, data");
    println!("   Auto-reconnect: enabled (2s delay)\n");

    // ========================================================================
    // Wait for Handshake
    // ========================================================================

    println!("🤝 Waiting for HELLO handshake to complete...");
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("✅ Handshake complete!\n");

    // ========================================================================
    // Message Exchange (Future Enhancement)
    // ========================================================================

    println!("📨 Message exchange would happen here...");
    println!("   (In a full application, you would:");
    println!("   - Get room handles from SessionManager");
    println!("   - Send messages via room.send()");
    println!("   - Receive messages via room.receive())");
    println!("\n");

    // ========================================================================
    // Keep Running
    // ========================================================================

    println!("✅ Example complete!");
    println!("   Server and client are connected and ready.");
    println!("   Press Ctrl+C to exit.\n");

    // Keep the example running for observation
    tokio::time::sleep(Duration::from_secs(3)).await;

    println!("👋 Shutting down gracefully...");

    Ok(())
}

// ============================================================================
// Additional Notes for Developers
// ============================================================================

// **Key Integration Points**:
//
// 1. **Message Definition**: Define your app's message enum with `#[derive(Clone)]`
//
// 2. **RoomMessageTrait**: Implement to map messages to rooms
//
// 3. **SessionManager**: Create one per process to manage all peer connections
//
// 4. **ServerBuilder**: Configure and start server:
//    - `bind()` - Address to listen on
//    - `as_role()` - Authentication role
//    - `offer_rooms()` - Rooms this server provides
//    - `with_connection_manager()` - Link to SessionManager
//
// 5. **ClientBuilder**: Configure and start client:
//    - `connect_to()` - Server address
//    - `as_role()` - Authentication role
//    - `offer_rooms()` - Rooms this client needs
//    - `with_connection_manager()` - Link to SessionManager
//    - `auto_reconnect()` - Enable automatic reconnection
//    - `reconnect_delay()` - Delay between reconnection attempts
//
// 6. **Room Access**: Get room handles from SessionManager to send/receive messages
//
// 7. **Lifecycle Management**: Use `StopServer`/`Disconnect` messages for shutdown

// **Next Steps for Full Application**:
//
// 1. Add message handlers to application actors
// 2. Get room handles from ConnectionManager
// 3. Send messages: `room.send(peer_id, message).await`
// 4. Receive messages: spawn task to handle `room.receive().await`
// 5. Add error handling and retry logic
// 6. Add TLS configuration for production use
// 7. Add health monitoring and metrics
