//! Internal messages for the three-actor CState pattern.
//!
//! These messages are used for communication between:
//! - CStateActor (MainActor) - Business logic
//! - CStateNetworkManager - Peer lifecycle orchestration
//! - CStateNetworkActor - Per-peer protocol translation
//!
//! These are INTERNAL messages and are NOT sent over the network.

use crate::network_messages::{CStateMessage, CollectorInfo};
use actix::prelude::*;
use zznet_api::types::PeerId;

// ============================================================================
// Inbound Messages (NetworkActor → MainActor)
// ============================================================================

/// A heartbeat message received from a collector peer.
///
/// NetworkActor translates CStateMessage::Heartbeat into this internal message
/// and forwards it to MainActor for business logic processing.
#[derive(Message)]
#[rtype(result = "()")]
pub struct InboundHeartbeat {
    /// The peer ID of the sender.
    pub peer_id: PeerId,
    /// The unique ID of the collector.
    pub collector_id: String,
    /// The uptime of the collector in seconds.
    pub uptime_secs: u64,
    /// The total number of pings sent by the collector.
    pub pings_sent: u64,
    /// The total number of pings received by the collector.
    pub pings_received: u64,
    /// The total number of batches sent by the collector.
    pub batches_sent: u64,
    /// The timestamp of the last configuration update.
    pub last_config_update_ms: u64,
    /// A unique nonce for the collector's connection.
    pub connection_nonce: u64,
}

/// A heartbeat acknowledgment received from the database.
///
/// NetworkActor translates CStateMessage::HeartbeatAck into this internal message.
#[derive(Message)]
#[rtype(result = "()")]
pub struct InboundHeartbeatAck {
    /// The peer ID of the sender (database).
    pub peer_id: PeerId,
    /// The timestamp of the heartbeat being acknowledged.
    pub timestamp_ms: u64,
    /// The server's time.
    pub server_time_ms: u64,
}

/// A query for the list of collectors from an admin peer.
///
/// NetworkActor translates CStateMessage::QueryCollectors into this internal message.
#[derive(Message)]
#[rtype(result = "()")]
pub struct InboundQueryCollectors {
    /// The peer ID of the requester (admin).
    pub peer_id: PeerId,
}

/// A collector list received from the database (for monitoring).
///
/// NetworkActor translates CStateMessage::CollectorList into this internal message.
#[derive(Message)]
#[rtype(result = "()")]
pub struct InboundCollectorList {
    /// The peer ID of the sender (database).
    pub peer_id: PeerId,
    /// The list of active collectors.
    pub collectors: Vec<CollectorInfo>,
}

/// Registration rejected message received from database.
///
/// NetworkActor translates CStateMessage::RegistrationRejected into this internal message.
#[derive(Message)]
#[rtype(result = "()")]
pub struct InboundRegistrationRejected {
    /// The peer ID of the sender (database).
    pub peer_id: PeerId,
    /// Human-readable reason for rejection.
    pub reason: String,
}

/// Unauthorized message received from database.
///
/// NetworkActor translates CStateMessage::Unauthorized into this internal message.
#[derive(Message)]
#[rtype(result = "()")]
pub struct InboundUnauthorized {
    /// The peer ID of the sender (database).
    pub peer_id: PeerId,
    /// Human-readable reason for denial.
    pub reason: String,
}

// ============================================================================
// Outbound Messages (MainActor → NetworkManager → NetworkActor)
// ============================================================================

/// Command to send a heartbeat acknowledgment to a specific peer.
///
/// MainActor sends this to NetworkManager, which forwards it to the appropriate NetworkActor.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SendHeartbeatAck {
    /// The peer ID to send the acknowledgment to.
    pub peer_id: PeerId,
    /// The timestamp of the heartbeat being acknowledged.
    pub timestamp_ms: u64,
    /// The server's time.
    pub server_time_ms: u64,
}

/// Command to send the collector list to a specific peer (admin).
///
/// MainActor sends this to NetworkManager, which forwards it to the appropriate NetworkActor.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SendCollectorList {
    /// The peer ID to send the list to.
    pub peer_id: PeerId,
    /// The list of active collectors.
    pub collectors: Vec<CollectorInfo>,
}

/// Command to send a registration rejection to a specific peer.
///
/// MainActor sends this to NetworkManager, which forwards it to the appropriate NetworkActor.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SendRegistrationRejected {
    /// The peer ID to send the rejection to.
    pub peer_id: PeerId,
    /// Human-readable reason for rejection.
    pub reason: String,
}

/// Command to send an unauthorized message to a specific peer.
///
/// MainActor sends this to NetworkManager, which forwards it to the appropriate NetworkActor.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SendUnauthorized {
    /// The peer ID to send the message to.
    pub peer_id: PeerId,
    /// Human-readable reason for denial.
    pub reason: String,
}

/// Command to broadcast a heartbeat to all database peers.
///
/// MainActor (in Collector role) sends this to NetworkManager, which broadcasts
/// to all connected NetworkActors.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct BroadcastHeartbeat {
    /// The collector ID.
    pub collector_id: String,
    /// The uptime of the collector in seconds.
    pub uptime_secs: u64,
    /// The total number of pings sent.
    pub pings_sent: u64,
    /// The total number of pings received.
    pub pings_received: u64,
    /// The total number of batches sent.
    pub batches_sent: u64,
    /// The timestamp of the last configuration update.
    pub last_config_update_ms: u64,
    /// A unique nonce for the collector's connection.
    pub connection_nonce: u64,
}

// ============================================================================
// NetworkManager → NetworkActor Messages
// ============================================================================

/// Generic message wrapper to send a network message to a specific peer.
///
/// NetworkManager uses this to forward outbound messages to the appropriate NetworkActor.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SendToNetwork {
    /// The network message to send.
    pub message: CStateMessage,
}

// ============================================================================
// NetworkManager Setup Messages
// ============================================================================

/// Message to set the NetworkManager address in the MainActor.
///
/// This is sent during the wiring phase to establish bidirectional communication.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetNetworkManager {
    /// The NetworkManager address.
    pub network_manager: Addr<crate::network_manager::CStateNetworkManager>,
}
