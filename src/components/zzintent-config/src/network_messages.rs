//! Network messages for the `intent-config` room.
//!
//! Typed, transport-agnostic messages exchanged between Database, Collector, and AdminClient.

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use zznet_api::RoomId;
use zznet_room::{DeserializationError, RoomMessageTrait, SerializationError};

/// Network messages for the intent-config room.
#[derive(Clone, Debug, Message, Serialize, Deserialize, PartialEq)]
#[rtype(result = "()")]
pub(crate) enum IntentConfigNetworkMsg {
    /// Admin request to change configuration (requires authorization).
    RequestConfigChange {
        /// Requester's peer ID (filled by routing layer).
        sender_peer_id: String,
        /// Targets to configure for pinging.
        targets: Vec<IpAddr>,
        /// Ping rate in packets/sec.
        ping_rate_pps: u64,
    },

    /// Query for current configuration.
    QueryCurrentConfig,

    /// Current configuration response.
    CurrentConfig {
        /// Targets to configure for pinging.
        targets: Vec<IpAddr>,
        /// Ping rate in packets/sec.
        ping_rate_pps: u64,
    },

    /// Keepalive heartbeat message.
    Heartbeat,

    /// Error response with human-readable reason.
    Error {
        /// Human-readable error description.
        reason: String,
    },
}

impl RoomMessageTrait for IntentConfigNetworkMsg {
    fn room_id(&self) -> RoomId {
        // All intent-config messages share the same room.
        RoomId::from("intent-config")
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        // Serialize via MessagePack for transport.
        log::debug!("[INTENT-CONFIG SEND] Serializing message: {:?}", self);
        let result =
            rmp_serde::to_vec(self).map_err(|e| SerializationError::MsgPackError(e.to_string()));
        if let Ok(ref bytes) = result {
            log::debug!("[INTENT-CONFIG SEND] Serialized to {} bytes", bytes.len());
        }
        result
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        if room_id.as_str() != "intent-config" {
            return Err(DeserializationError::UnknownRoom(room_id.clone()));
        }

        log::debug!(
            "[INTENT-CONFIG RECV] Deserializing {} bytes from room {:?}",
            bytes.len(),
            room_id
        );
        let result = rmp_serde::from_slice(bytes)
            .map_err(|e| DeserializationError::MsgPackError(e.to_string()));
        if let Ok(ref msg) = result {
            log::debug!("[INTENT-CONFIG RECV] Deserialized message: {:?}", msg);
        } else {
            log::warn!("[INTENT-CONFIG RECV] Deserialization failed");
        }
        result
    }
}
