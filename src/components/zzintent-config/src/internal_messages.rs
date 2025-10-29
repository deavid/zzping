//! Internal messages for three-actor pattern communication
//!
//! These messages are used for communication between:
//! - IntentConfigActor (Main Actor - business logic)
//! - IntentConfigNetworkManager (Manager Actor - peer lifecycle)
//! - IntentConfigNetworkActor (Network Actor - per-peer protocol)
//!
//! These are NOT exposed in the public API - they are internal implementation details
//! of the three-actor architecture.

use crate::messages::IntentConfigData;
use actix::prelude::*;
use std::net::IpAddr;
use zznet_api::types::PeerId;

// ============================================================================
// Messages: MainActor → NetworkManager
// ============================================================================

/// Request to broadcast a config update to all connected peers
///
/// Sent by MainActor when configuration changes and needs to be
/// propagated to all connected peers (Database role only).
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct BroadcastConfigUpdate {
    /// The configuration data to broadcast
    pub config: IntentConfigData,
}

/// Request to send an error message to a specific peer
///
/// Sent by MainActor when an operation fails and the peer needs
/// to be notified (e.g., authorization failure, persistence error).
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct SendErrorToPeer {
    /// The ID of the peer to send the error to
    pub peer_id: PeerId,
    /// The error message to send
    pub error_message: String,
}

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

// ============================================================================
// Messages: NetworkManager → NetworkActor
// ============================================================================

/// Command to send a config update to the peer
///
/// Sent by NetworkManager when broadcasting config updates.
/// NetworkActor serializes and sends via Room<T>.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct SendConfigUpdateToPeer {
    /// The configuration data to send
    pub config: IntentConfigData,
}

/// Command to send an error message to the peer
///
/// Sent by NetworkManager when an error needs to be communicated.
/// NetworkActor serializes and sends via Room<T>.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct SendErrorMessageToPeer {
    /// The error message to send
    pub error_message: String,
}

/// Command to send the current config in response to GetConfig request
///
/// Sent by NetworkManager in response to InboundGetConfigRequest.
/// NetworkActor serializes and sends via Room<T>.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct SendConfigResponseToPeer {
    /// The configuration data to send
    pub config: IntentConfigData,
}

// ============================================================================
// Message Flow Documentation
// ============================================================================

// # Message Flow: Config Update (Database → Collectors)
//
// ```text
// 1. External API → MainActor.UpdateConfig
// 2. MainActor validates, persists, broadcasts locally
// 3. MainActor → BroadcastConfigUpdate → NetworkManager
// 4. NetworkManager iterates network_actors HashMap
// 5. NetworkManager → SendConfigUpdateToPeer → NetworkActor[each peer]
// 6. NetworkActor serializes IntentConfigNetworkMsg::ConfigUpdate
// 7. NetworkActor → Room<T>.send() → Peer
// ```
//
// # Message Flow: Config Change Request (Collector → Database)
//
// ```text
// 1. Peer → Room<T> → NetworkActor.IntentConfigNetworkMsg::RequestConfigChange
// 2. NetworkActor → InboundConfigChangeRequest → NetworkManager
// 3. NetworkManager → PeerManager.get_peer_role(peer_id)
// 4. NetworkManager checks authorization (role = ClientAdmin?)
// 5a. If authorized:
//     - NetworkManager → NetworkConfigChangeRequest(authorized=true) → MainActor
//     - MainActor validates, persists, broadcasts
//     - MainActor → BroadcastConfigUpdate → NetworkManager
// 5b. If not authorized:
//     - NetworkManager → SendErrorMessageToPeer("unauthorized") → NetworkActor
//     - NetworkActor → Room<T> → Error message to peer
// ```
//
// # Message Flow: Get Config Request (Collector → Database)
//
// ```text
// 1. Peer → Room<T> → NetworkActor.IntentConfigNetworkMsg::QueryCurrentConfig
// 2. NetworkActor → InboundGetConfigRequest → NetworkManager
// 3. NetworkManager → MainActor.GetCurrentConfig
// 4. MainActor → returns IntentConfigData → NetworkManager
// 6. NetworkActor serializes IntentConfigNetworkMsg::CurrentConfig
// 7. NetworkActor → Room<T>.send() → Peer
// ```

// ============================================================================
// Setup Messages (Builder → MainActor)
// ============================================================================

/// Set the NetworkManager address after actor creation
///
/// Sent by Builder after creating both MainActor and NetworkManager
/// to establish the bidirectional link between them.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetNetworkManager {
    /// The address of the network manager
    pub network_manager: Addr<crate::network_manager::IntentConfigNetworkManager>,
}
