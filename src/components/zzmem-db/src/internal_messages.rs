//! Internal messages for MemDB three-actor pattern.
//!
//! This module defines the messages used for communication between:
//! - MainActor (MemDBActor): Pure business logic
//! - NetworkManager (MemDBNetworkManager): Peer lifecycle and routing
//! - NetworkActor (MemDBNetworkActor): Per-peer protocol translation
//!
//! ## Message Flow
//!
//! ### Inbound (Network → MainActor):
//! ```text
//! Network → NetworkActor → MainActor
//! MemDBMessage::SubmitBatch → InboundSubmitBatch
//! MemDBMessage::Query → InboundQuery
//! MemDBMessage::BatchAck → InboundBatchAck
//! MemDBMessage::QueryResponse → InboundQueryResponse
//! ```
//!
//! ### Outbound (MainActor → Network):
//! ```text
//! MainActor → NetworkManager → NetworkActor → Network
//! SendBatchAck → MemDBMessage::BatchAck
//! SendQueryResponse → MemDBMessage::QueryResponse
//! SendSubmitBatch → MemDBMessage::SubmitBatch
//! ```

use crate::network_messages::{PingResult, StoredPingResult};
use actix::prelude::*;
use zznet_api::types::PeerId;

// ============================================================================
// INBOUND MESSAGES (Network → MainActor) - Request/Reply Pattern
// ============================================================================

/// Database received batch of ping results from Collector.
///
/// Sent by: NetworkActor (translates MemDBMessage::SubmitBatch)
/// Handled by: MainActor (Database role)
/// Response: BatchAckResponse or error string
///
/// NetworkActor now awaits response and sends directly to room_actor
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<BatchAckResponse, String>")]
pub struct InboundSubmitBatch {
    /// Peer ID of the collector who sent the batch
    pub peer_id: PeerId,
    /// Timestamp when the batch was created (milliseconds since epoch)
    pub timestamp_ms: u64,
    /// The ping results in this batch
    pub results: Vec<PingResult>,
}

/// Response to a successful batch submission
#[derive(Debug, Clone)]
pub struct BatchAckResponse {
    /// Number of results that were received and stored
    pub received_count: usize,
    /// Timestamp when the batch was acknowledged
    pub timestamp_ms: u64,
}

/// Database received query request from Admin client.
///
/// Sent by: NetworkActor (translates MemDBMessage::Query)
/// Handled by: MainActor (Database role)
/// Response: QueryResponse or error string
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<Vec<StoredPingResult>, String>")]
pub struct InboundQuery {
    /// Peer ID of the admin client who sent the query
    pub peer_id: PeerId,
    /// Target host to query
    pub target: String,
    /// Start time for query (milliseconds since epoch)
    pub from_ms: u64,
    /// End time for query (milliseconds since epoch)
    pub to_ms: u64,
}

/// Collector received batch acknowledgment from Database.
///
/// This message is still sent by MainActor but goes to NetworkActor
/// (not through NetworkManager). NetworkActor sends it directly to room_actor.
/// Stored in MainActor's peer tracking for async broadcast capability.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct InboundBatchAck {
    /// Peer ID of the database who sent the ack
    pub peer_id: PeerId,
    /// Number of results that were received
    pub received_count: usize,
    /// Timestamp when the batch was acknowledged
    pub timestamp_ms: u64,
}

/// Admin client received query response from Database.
///
/// This message is still sent by MainActor but goes to NetworkActor
/// (not through NetworkManager). NetworkActor sends it directly to room_actor.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct InboundQueryResponse {
    /// Peer ID of the database who sent the response
    pub peer_id: PeerId,
    /// The query results
    pub results: Vec<StoredPingResult>,
}

// ============================================================================
// Outbound batch notification from MainActor
// ============================================================================
// When MainActor has a batch ready to send to Database peers,
// it creates a SubmitBatch network message that should be sent to all Database peers.
// For Collector role: MainActor is ready to send batch
// NetworkActors that have Database peers can listen for this and route to their RoomActors

/// Notification that MainActor has a batch ready to send (Collector role only)
///
/// Used for Collector→Database batch transmission
/// When MainActor has buffered enough results, it sends this message to NetworkManager
/// which broadcasts it to all connected Database peers via RoomActor
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct BatchReadyToSend {
    /// Timestamp when the batch was created
    pub timestamp_ms: u64,
    /// The ping results ready to send
    pub results: Vec<crate::network_messages::PingResult>,
}
