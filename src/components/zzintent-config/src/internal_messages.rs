//! Internal messages for the intent-config three-actor implementation.

use crate::messages::IntentConfigData;
use actix::prelude::*;
use std::net::IpAddr;
use zznet_api::types::PeerId;

// Messages: NetworkActor → NetworkManager

/// Peer-initiated request to change intent configuration.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub(crate) struct InboundConfigChangeRequest {
    /// The ID of the peer making the request
    pub peer_id: PeerId,
    /// The target IP addresses for ping
    pub targets: Vec<IpAddr>,
    /// The ping rate in packets per second
    pub ping_rate_pps: u64,
}

/// Peer request to fetch the current intent configuration.
#[derive(Message, Debug, Clone)]
#[rtype(result = "IntentConfigData")]
pub(crate) struct InboundGetConfigRequest {
    /// The ID of the peer making the request
    pub peer_id: PeerId,
}

// Messages: NetworkManager → MainActor

/// Notification to MainActor that a peer requested a configuration change.
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
