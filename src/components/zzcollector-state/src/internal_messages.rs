//! Internal messages for the three-actor CState pattern.
//!
//! These messages are used for communication between:
//! - CStateActor (MainActor) - Business logic
//! - GenericNetworkManager - Peer lifecycle orchestration
//! - CStateNetworkActor - Per-peer protocol translation
//!
//! These are INTERNAL messages and are NOT sent over the network.

use crate::network_messages::CollectorInfo;
use actix::prelude::*;
use zznet_api::PeerId;

// ============================================================================
// Inbound Messages (NetworkActor → MainActor)
// ============================================================================

/// Response data for heartbeat acknowledgment
#[derive(Debug, Clone)]
pub struct HeartbeatAckResponse {
    /// The timestamp of the heartbeat being acknowledged
    pub timestamp_ms: u64,
    /// The server's time
    pub server_time_ms: u64,
    /// Optional rejection reason if registration was denied
    pub rejection: Option<String>,
}

/// A heartbeat message received from a collector peer.
///
/// Now returns Result with acknowledgment or rejection
use crate::network_messages::CStateMessage;

#[derive(Message)]
#[rtype(result = "Result<HeartbeatAckResponse, String>")]
pub(crate) struct InboundHeartbeat {
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
    /// The recipient for sending messages back to the network actor.
    pub recipient: Recipient<CStateMessage>,
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
#[derive(Message)]
#[rtype(result = "Result<Vec<CollectorInfo>, String>")]
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

/// Inbound: Prepare to swap command received from the database.
#[derive(Message)]
#[rtype(result = "()")]
pub(crate) struct InboundPrepareToSwap {
    pub peer_id: PeerId,
    pub swap_time_ms: u64,
}

/// Inbound: Set mastership command received from the database.
#[derive(Message)]
#[rtype(result = "()")]
pub(crate) struct InboundSetMastership {
    pub peer_id: PeerId,
    pub is_primary: bool,
}

/// Internal command to execute the mastership swap.
#[derive(Message)]
#[rtype(result = "()")]
pub(crate) struct HandoffOrder {
    /// The ID of the collector being handed off.
    pub collector_id: String,
    /// The recipient of the old primary collector.
    pub old_recipient: Recipient<CStateMessage>,
    /// The recipient of the new primary collector.
    pub new_recipient: Recipient<CStateMessage>,
    /// The connection nonce of the new primary collector.
    pub new_nonce: u64,
}
