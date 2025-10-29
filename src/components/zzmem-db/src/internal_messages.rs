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
// INBOUND MESSAGES (Network → MainActor)
// ============================================================================

/// Database received batch of ping results from Collector.
///
/// Sent by: NetworkActor (translates MemDBMessage::SubmitBatch)
/// Handled by: MainActor (Database role)
///
/// Flow:
/// 1. NetworkActor receives MemDBMessage::SubmitBatch from network
/// 2. Translates to InboundSubmitBatch with peer_id
/// 3. MainActor stores results and sends SendBatchAck
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct InboundSubmitBatch {
    /// Peer ID of the collector who sent the batch
    pub peer_id: PeerId,
    /// Timestamp when the batch was created (milliseconds since epoch)
    pub timestamp_ms: u64,
    /// The ping results in this batch
    pub results: Vec<PingResult>,
}

/// Database received query request from Admin client.
///
/// Sent by: NetworkActor (translates MemDBMessage::Query)
/// Handled by: MainActor (Database role)
///
/// Flow:
/// 1. NetworkActor receives MemDBMessage::Query from network
/// 2. Translates to InboundQuery with peer_id
/// 3. MainActor queries storage and sends SendQueryResponse
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
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
/// Sent by: NetworkActor (translates MemDBMessage::BatchAck)
/// Handled by: MainActor (Collector role)
///
/// Flow:
/// 1. NetworkActor receives MemDBMessage::BatchAck from network
/// 2. Translates to InboundBatchAck with peer_id
/// 3. MainActor clears outstanding batch and updates metrics
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
/// Sent by: NetworkActor (translates MemDBMessage::QueryResponse)
/// Handled by: MainActor (Admin role, future use)
///
/// Flow:
/// 1. NetworkActor receives MemDBMessage::QueryResponse from network
/// 2. Translates to InboundQueryResponse with peer_id
/// 3. MainActor processes query results (future: forward to UI)
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct InboundQueryResponse {
    /// Peer ID of the database who sent the response
    pub peer_id: PeerId,
    /// The query results
    pub results: Vec<StoredPingResult>,
}

// ============================================================================
// OUTBOUND MESSAGES (MainActor → NetworkManager → NetworkActor → Network)
// ============================================================================

/// Request to send batch acknowledgment to specific collector peer.
///
/// Sent by: MainActor (Database role, after storing batch)
/// Handled by: NetworkManager (routes to specific peer's NetworkActor)
///
/// Flow:
/// 1. MainActor receives InboundSubmitBatch
/// 2. Stores results in storage
/// 3. Sends SendBatchAck to NetworkManager
/// 4. NetworkManager routes to peer's NetworkActor
/// 5. NetworkActor translates to MemDBMessage::BatchAck and sends
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct SendBatchAck {
    /// Target peer ID (the collector who sent the batch)
    pub peer_id: PeerId,
    /// Number of results that were received
    pub received_count: usize,
    /// Timestamp when the batch was acknowledged
    pub timestamp_ms: u64,
}

/// Request to send query response to specific admin peer.
///
/// Sent by: MainActor (Database role, after querying storage)
/// Handled by: NetworkManager (routes to specific peer's NetworkActor)
///
/// Flow:
/// 1. MainActor receives InboundQuery
/// 2. Queries storage for results
/// 3. Sends SendQueryResponse to NetworkManager
/// 4. NetworkManager routes to peer's NetworkActor
/// 5. NetworkActor translates to MemDBMessage::QueryResponse and sends
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct SendQueryResponse {
    /// Target peer ID (the admin who sent the query)
    pub peer_id: PeerId,
    /// The query results to send
    pub results: Vec<StoredPingResult>,
}

/// Request to send batch of ping results to database peer.
///
/// Sent by: MainActor (Collector role, when buffer is full)
/// Handled by: NetworkManager (routes to database peer's NetworkActor)
///
/// Flow:
/// 1. MainActor receives StorePingResult (from Pinger component)
/// 2. Buffers results until buffer is full
/// 3. Sends SendSubmitBatch to NetworkManager
/// 4. NetworkManager routes to database peer's NetworkActor
/// 5. NetworkActor translates to MemDBMessage::SubmitBatch and sends
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct SendSubmitBatch {
    /// Target peer ID (the database peer)
    pub peer_id: PeerId,
    /// Timestamp when the batch was created
    pub timestamp_ms: u64,
    /// The ping results to send
    pub results: Vec<PingResult>,
}

// ============================================================================
// SYSTEM MESSAGES (Wiring & Internal Communication)
// ============================================================================

/// Internal message for NetworkActor to send a message over the network.
///
/// Sent by: NetworkManager (after routing decision)
/// Handled by: NetworkActor (sends via Room<T>)
///
/// This message wraps the actual MemDBMessage and triggers the network send.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct SendToNetwork {
    /// The network message to send (already in wire format)
    pub message: crate::network_messages::MemDBMessage,
}

/// Wire the NetworkManager to the MainActor after creation.
///
/// Sent by: Builder (during system initialization)
/// Handled by: MainActor (stores NetworkManager address)
///
/// This message completes the wiring between MainActor and NetworkManager,
/// enabling MainActor to send outbound message requests.
#[derive(Message, Debug)]
#[rtype(result = "()")]
pub struct SetNetworkManager {
    /// The NetworkManager actor address
    pub network_manager: Addr<crate::network_manager::MemDBNetworkManager>,
}
