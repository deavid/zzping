//! Internal messages for three-actor pattern communication
//!
//! These messages are used for communication between:
//! - IntentConfigActor (Main Actor - business logic)
//! - IntentConfigNetworkManager (Manager Actor - peer lifecycle)
//! - IntentConfigNetworkActor (Translator Actor - per-peer protocol)
//!
//! These are NOT exposed in the public API - they are internal implementation details
//! of the three-actor architecture.

use crate::messages::IntentConfigData;
use actix::prelude::*;
use std::net::IpAddr;
use zznet_api::types::PeerId;

// ============================================================================
// Messages: NetworkActor → NetworkManager
// ============================================================================

/// Inbound config change request from a peer
///
/// Sent by NetworkActor when it receives a RequestConfigChange message
/// from its peer. NetworkManager is responsible for:
/// 1. Querying PeerManager for authorization (role check)
/// 2. Forwarding to MainActor if authorized
/// 3. Sending error response if not authorized
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct InboundConfigChangeRequest {
    /// The ID of the peer making the request
    pub peer_id: PeerId,
    /// The target IP addresses for ping
    pub targets: Vec<IpAddr>,
    /// The ping rate in packets per second
    pub ping_rate_pps: u64,
}

/// Inbound request to get current configuration
///
/// Sent by NetworkActor when it receives a GetConfig message from its peer.
/// NetworkManager forwards to MainActor for processing.
#[derive(Message, Debug, Clone)]
#[rtype(result = "IntentConfigData")]
pub struct InboundGetConfigRequest {
    /// The ID of the peer making the request
    pub peer_id: PeerId,
}

// ============================================================================
// Messages: NetworkManager → MainActor
// ============================================================================

/// Notification that a peer has requested a configuration change
///
/// Sent by NetworkManager after authorization check passes.
/// MainActor is responsible for:
/// 1. Validating the new configuration
/// 2. Persisting to disk (Database role)
/// 3. Broadcasting to local subscribers
/// 4. Requesting network broadcast via NetworkManager
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct NetworkConfigChangeRequest {
    /// The ID of the peer making the request
    pub peer_id: PeerId,
    /// The target IP addresses for ping
    pub targets: Vec<IpAddr>,
    /// The ping rate in packets per second
    pub ping_rate_pps: u64,
    /// Whether the peer is authorized to make changes
    pub authorized: bool,
}
