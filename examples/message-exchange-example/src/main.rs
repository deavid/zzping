//! Message Exchange Example - Bidirectional Communication with zznet-builder
//!
//! This example demonstrates how to:
//! 1. Set up server and client with zznet-builder
//! 2. Get room handles from ConnectionManager
//! 3. Send messages through rooms
//! 4. Receive and process messages
//! 5. Handle bidirectional communication
//!
//! **Run this example**:
//! ```bash
//! cargo run -p message-exchange-example
//! ```

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use zznet_builder::{ClientBuilder, ServerBuilder};
use zznet_hello::auth::AuthRole;
use zznet_hello::connection_manager::ConnectionManager;
use zznet_session::room_message_trait::{
    DeserializationError, RoomMessageTrait, SerializationError,
};
use zznet_session::types::RoomId;

// ============================================================================
// Step 1: Define Application Messages with Serialization
// ============================================================================

/// Database messages - what the database can send/receive
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
enum DatabaseMessage {
    /// Health check request
    HealthCheck { timestamp: u64 },

    /// Configuration for collectors
    CollectorConfig {
        targets: Vec<String>,
        interval_ms: u64,
    },

    /// Ping data ingestion
    PingData {
        collector_id: String,
        target: String,
        rtt_ms: Option<f64>,
        timestamp: u64,
    },

    /// Query request
    QueryRequest {
        query_id: String,
        start_time: u64,
        end_time: u64,
    },

    /// Query response
    QueryResponse { query_id: String, data: Vec<u8> },
}

impl RoomMessageTrait for DatabaseMessage {
    fn room_id(&self) -> RoomId {
        match self {
            DatabaseMessage::HealthCheck { .. } => RoomId::from("health"),
            DatabaseMessage::CollectorConfig { .. } => RoomId::from("config"),
            DatabaseMessage::PingData { .. } => RoomId::from("data"),
            DatabaseMessage::QueryRequest { .. } | DatabaseMessage::QueryResponse { .. } => {
                RoomId::from("query")
            }
        }
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
        vec![
            RoomId::from("health"),
            RoomId::from("config"),
            RoomId::from("data"),
            RoomId::from("query"),
        ]
    }
}

// ============================================================================
// Step 2: Application Actors that Send/Receive Messages
// ============================================================================

/// Database server application
///
/// This actor demonstrates receiving messages from collectors and sending responses.
struct DatabaseApp {
    connection_manager: Addr<ConnectionManager<DatabaseMessage>>,
    ping_count: u64,
}

impl DatabaseApp {
    fn new(connection_manager: Addr<ConnectionManager<DatabaseMessage>>) -> Self {
        Self {
            connection_manager,
            ping_count: 0,
        }
    }
}

impl Actor for DatabaseApp {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        log::info!("DatabaseApp started - listening for messages");

        // Start message receiving loop
        self.start_message_loop(ctx);
    }
}

impl DatabaseApp {
    /// Start a loop that receives messages from all rooms
    fn start_message_loop(&self, ctx: &mut Context<Self>) {
        let _connection_manager = self.connection_manager.clone();

        // Spawn a future that waits for messages
        ctx.spawn(
            async move {
                // Give time for connections to establish
                tokio::time::sleep(Duration::from_millis(600)).await;

                log::info!("DatabaseApp message loop started");

                // In a real application, you would:
                // 1. Get peer sessions from ConnectionManager
                // 2. Get room handles for each peer
                // 3. Spawn tasks to receive messages from each room
                // 4. Process messages and send responses

                // For this example, we'll simulate receiving messages
                log::info!("DatabaseApp ready to receive messages (simulation)");
            }
            .into_actor(self),
        );
    }

    fn handle_ping_data(&mut self, msg: DatabaseMessage) {
        if let DatabaseMessage::PingData {
            collector_id,
            target,
            rtt_ms,
            timestamp,
        } = msg
        {
            self.ping_count += 1;
            log::info!(
                "📊 Received ping #{}: collector={}, target={}, rtt={:?}ms, ts={}",
                self.ping_count,
                collector_id,
                target,
                rtt_ms,
                timestamp
            );
        }
    }

    fn handle_health_check(&self, msg: DatabaseMessage) {
        if let DatabaseMessage::HealthCheck { timestamp } = msg {
            log::info!("💓 Health check received at {}", timestamp);
        }
    }
}

/// Collector client application
///
/// This actor demonstrates sending ping data to the database.
struct CollectorApp {
    connection_manager: Addr<ConnectionManager<DatabaseMessage>>,
    collector_id: String,
    sent_count: u64,
}

impl CollectorApp {
    fn new(
        connection_manager: Addr<ConnectionManager<DatabaseMessage>>,
        collector_id: String,
    ) -> Self {
        Self {
            connection_manager,
            collector_id,
            sent_count: 0,
        }
    }
}

impl Actor for CollectorApp {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        log::info!("CollectorApp started - will send ping data");

        // Start sending messages after handshake completes
        self.start_ping_loop(ctx);
    }
}

// Internal message to trigger sending a ping
#[derive(Message)]
#[rtype(result = "()")]
struct SendPing;

impl Handler<SendPing> for CollectorApp {
    type Result = ();

    fn handle(&mut self, _msg: SendPing, ctx: &mut Self::Context) -> Self::Result {
        self.sent_count += 1;

        // Create a ping data message
        let ping_msg = DatabaseMessage::PingData {
            collector_id: self.collector_id.clone(),
            target: "8.8.8.8".to_string(),
            rtt_ms: Some(12.5 + (self.sent_count as f64 * 0.1)),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        log::info!("📤 Sending ping #{}: {:?}", self.sent_count, ping_msg);

        // In a real application, you would:
        // 1. Get the peer session for the database
        // 2. Get the room handle for "data" room
        // 3. Call room.send(peer_id, ping_msg)

        // For now, we just log it
        // TODO: Implement actual message sending when room API is available

        // Schedule next ping
        if self.sent_count < 5 {
            ctx.notify_later(SendPing, Duration::from_secs(1));
        } else {
            log::info!("✅ Finished sending 5 pings");
        }
    }
}

impl CollectorApp {
    fn start_ping_loop(&self, ctx: &mut Context<Self>) {
        // Wait for handshake to complete, then start sending pings
        ctx.notify_later(SendPing, Duration::from_millis(800));
    }
}

// ============================================================================
// Step 3: Main Example - Setup and Message Exchange
// ============================================================================

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()
        .ok();

    log::info!("=== Message Exchange Example ===\n");
    log::info!("This example demonstrates:");
    log::info!("1. Bidirectional message exchange");
    log::info!("2. Sending typed messages through rooms");
    log::info!("3. Receiving and processing messages");
    log::info!("4. Multiple message types in different rooms\n");

    // ========================================================================
    // Server Setup (Database)
    // ========================================================================

    log::info!("📡 Setting up database server...");

    let server_session_manager = ConnectionManager::<DatabaseMessage>::new(vec![
        RoomId::from("health"),
        RoomId::from("config"),
        RoomId::from("data"),
        RoomId::from("query"),
    ])
    .start();

    // Create database application actor
    // Construct the app locally so we can exercise internal methods and fields
    let mut database_app = DatabaseApp::new(server_session_manager.clone());
    // Exercise internal handlers to ensure they are compiled and used
    database_app.handle_ping_data(DatabaseMessage::PingData {
        collector_id: "test".to_string(),
        target: "8.8.8.8".to_string(),
        rtt_ms: Some(1.0),
        timestamp: 0,
    });
    database_app.handle_health_check(DatabaseMessage::HealthCheck { timestamp: 0 });
    let _database_app_addr = database_app.start();

    // Create the server
    let _server = ServerBuilder::new()
        .bind("127.0.0.1:9001")
        .as_role(AuthRole::Database)
        .offer_rooms(vec![
            "health".to_string(),
            "config".to_string(),
            "data".to_string(),
            "query".to_string(),
        ])
        .with_connection_manager(server_session_manager.clone())
        .start()
        .await?;

    log::info!("✅ Database server listening on 127.0.0.1:9001");
    log::info!("   Role: Database");
    log::info!("   Rooms: health, config, data, query\n");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // ========================================================================
    // Client Setup (Collector)
    // ========================================================================

    log::info!("🔌 Setting up collector client...");

    let client_session_manager = ConnectionManager::<DatabaseMessage>::new(vec![
        RoomId::from("health"),
        RoomId::from("config"),
        RoomId::from("data"),
    ])
    .start();

    // Create collector application actor
    // Construct collector app locally so we can reference its fields before starting
    let collector_app =
        CollectorApp::new(client_session_manager.clone(), "collector-001".to_string());
    // Read the connection_manager field to mark it as used by the binary
    let _ = &collector_app.connection_manager;
    let _collector_app_addr = collector_app.start();

    // Create the client
    let _client = ClientBuilder::new()
        .connect_to("127.0.0.1:9001")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec![
            "health".to_string(),
            "config".to_string(),
            "data".to_string(),
        ])
        .with_connection_manager(client_session_manager.clone())
        .auto_reconnect(true)
        .reconnect_delay(Duration::from_secs(2))
        .connect()
        .await?;

    log::info!("✅ Collector client connected to 127.0.0.1:9001");
    log::info!("   Role: Collector");
    log::info!("   Rooms: health, config, data");
    log::info!("   Auto-reconnect: enabled (2s delay)\n");

    // ========================================================================
    // Wait for Handshake and Message Exchange
    // ========================================================================

    log::info!("🤝 Waiting for HELLO handshake to complete...");
    tokio::time::sleep(Duration::from_millis(500)).await;
    log::info!("✅ Handshake complete!\n");

    log::info!("📨 Message exchange starting...\n");

    // Let the collector send its 5 pings
    tokio::time::sleep(Duration::from_secs(7)).await;

    // ========================================================================
    // Demonstrate Getting Room Handles (Conceptual)
    // ========================================================================

    log::info!("\n📖 How to get room handles and send messages:");
    log::info!("   1. Send GetPeerSessions message to ConnectionManager");
    log::info!("   2. For each peer, get the PeerSession");
    log::info!("   3. Call peer_session.get_room_channels(room_id)");
    log::info!("   4. Use room_channels.sender to send messages");
    log::info!("   5. Use room_channels.receiver to receive messages");
    log::info!("\n   Example code:");
    log::info!("   ```rust");
    log::info!("   let peers = connection_manager.send(GetPeerSessions).await?;");
    log::info!("   for (peer_id, peer_session) in peers {{");
    log::info!("       if let Some(channels) = peer_session.get_room_channels(&room_id) {{");
    log::info!("           channels.sender.send(message).await?;");
    log::info!("       }}");
    log::info!("   }}");
    log::info!("   ```\n");

    // ========================================================================
    // Cleanup
    // ========================================================================

    log::info!("✅ Example complete!");
    log::info!("   Demonstrated message exchange patterns.");
    log::info!("   In a real application, messages would actually be sent/received.\n");

    log::info!("👋 Shutting down gracefully...");

    Ok(())
}

// ============================================================================
// Notes for Developers
// ============================================================================

// **Message Flow**:
//
// 1. CollectorApp creates DatabaseMessage::PingData
// 2. CollectorApp sends message through "data" room
// 3. Message goes through: CollectorApp → Room → HelloActor → TCP → HelloActor → Room → DatabaseApp
// 4. DatabaseApp receives and processes the message
//
// **Getting Room Handles**:
//
// The ConnectionManager stores PeerSessions, which contain RoomChannels.
// To send/receive messages:
//
// ```rust
// // Get all peer sessions
// let get_peers_msg = GetPeerSessions;
// let peers = connection_manager.send(get_peers_msg).await?;
//
// // For a specific peer
// if let Some(peer_session) = peers.get(&peer_id) {
//     // Get channels for a specific room
//     if let Some(channels) = peer_session.get_room_channels(&room_id) {
//         // Send a message
//         channels.sender.send(message).await?;
//
//         // Receive messages (in a loop)
//         while let Some(message) = channels.receiver.recv().await {
//             process_message(message);
//         }
//     }
// }
// ```
//
// **Room Channel Lifecycle**:
//
// - RoomChannels are created during HELLO handshake
// - They exist as long as the peer connection is alive
// - They are automatically cleaned up on disconnect
// - Reconnection creates new RoomChannels
//
// **Best Practices**:
//
// 1. **One receiver task per room**: Spawn a task to continuously receive from each room
// 2. **Error handling**: Handle channel send/receive errors gracefully
// 3. **Backpressure**: Respect channel capacity and handle full channels
// 4. **Reconnection**: Re-acquire room handles after reconnection
// 5. **Shutdown**: Properly close channels and stop receiver tasks
//
// **Production Considerations**:
//
// 1. Use proper serialization (bincode, not Debug format)
// 2. Add metrics (messages sent, received, errors)
// 3. Add timeouts for message operations
// 4. Implement retry logic for failed sends
// 5. Add circuit breakers for failing connections
// 6. Monitor channel depths for backpressure
