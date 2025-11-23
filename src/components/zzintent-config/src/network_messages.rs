//! Network messages for the `intent-config` room.
//!
//! Typed, transport-agnostic messages exchanged between Database, Collector, and AdminClient.

use actix::prelude::*;
use log;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use zznet_api::types::RoomId;
use zznet_room::room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError};

/// Network messages for the intent-config room.
#[derive(Clone, Debug, Message, Serialize, Deserialize, PartialEq)]
#[rtype(result = "()")]
pub enum IntentConfigNetworkMsg {
    /// Admin request to change configuration (requires authorization).
    RequestConfigChange {
        /// Requester's peer ID (filled by routing layer).
        sender_peer_id: String,
        /// Targets to configure for pinging.
        targets: Vec<IpAddr>,
        /// Ping rate in packets/sec.
        ping_rate_pps: u64,
    },

    /// Configuration update from Database to Collectors.
    ConfigUpdate {
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

impl IntentConfigNetworkMsg {
    /// Create a `ConfigUpdate` message for internal distribution.
    pub fn config_update(targets: Vec<IpAddr>, ping_rate_pps: u64) -> Self {
        Self::ConfigUpdate {
            targets,
            ping_rate_pps,
        }
    }

    /// Create a `CurrentConfig` message.
    pub fn current_config(targets: Vec<IpAddr>, ping_rate_pps: u64) -> Self {
        Self::CurrentConfig {
            targets,
            ping_rate_pps,
        }
    }

    /// Create an `Error` message.
    pub fn error(reason: impl Into<String>) -> Self {
        Self::Error {
            reason: reason.into(),
        }
    }

    /// Returns true if the message carries configuration data.
    pub fn has_config_data(&self) -> bool {
        matches!(self, Self::ConfigUpdate { .. } | Self::CurrentConfig { .. })
    }

    /// Extract configuration data when present.
    pub fn config_data(&self) -> Option<(Vec<IpAddr>, u64)> {
        match self {
            Self::ConfigUpdate {
                targets,
                ping_rate_pps,
            }
            | Self::CurrentConfig {
                targets,
                ping_rate_pps,
            } => Some((targets.clone(), *ping_rate_pps)),
            _ => None,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_update_creation() {
        let targets = vec!["8.8.8.8".parse().unwrap(), "1.1.1.1".parse().unwrap()];
        let msg = IntentConfigNetworkMsg::config_update(targets.clone(), 100);

        match msg {
            IntentConfigNetworkMsg::ConfigUpdate {
                targets: t,
                ping_rate_pps,
            } => {
                assert_eq!(t, targets);
                assert_eq!(ping_rate_pps, 100);
            }
            _ => panic!("Expected ConfigUpdate"),
        }
    }

    #[test]
    fn test_current_config_creation() {
        let targets = vec!["8.8.8.8".parse().unwrap()];
        let msg = IntentConfigNetworkMsg::current_config(targets.clone(), 50);

        match msg {
            IntentConfigNetworkMsg::CurrentConfig {
                targets: t,
                ping_rate_pps,
            } => {
                assert_eq!(t, targets);
                assert_eq!(ping_rate_pps, 50);
            }
            _ => panic!("Expected CurrentConfig"),
        }
    }

    #[test]
    fn test_error_creation() {
        let msg = IntentConfigNetworkMsg::error("Test error");
        match msg {
            IntentConfigNetworkMsg::Error { reason } => {
                assert_eq!(reason, "Test error");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_has_config_data() {
        let update = IntentConfigNetworkMsg::config_update(vec![], 10);
        assert!(update.has_config_data());

        let current = IntentConfigNetworkMsg::current_config(vec![], 10);
        assert!(current.has_config_data());

        let query = IntentConfigNetworkMsg::QueryCurrentConfig;
        assert!(!query.has_config_data());

        let heartbeat = IntentConfigNetworkMsg::Heartbeat;
        assert!(!heartbeat.has_config_data());

        let error = IntentConfigNetworkMsg::error("test");
        assert!(!error.has_config_data());
    }

    #[test]
    fn test_config_data_extraction() {
        let targets = vec!["8.8.8.8".parse().unwrap()];
        let update = IntentConfigNetworkMsg::config_update(targets.clone(), 100);

        let (extracted_targets, rate) = update.config_data().unwrap();
        assert_eq!(extracted_targets, targets);
        assert_eq!(rate, 100);

        let query = IntentConfigNetworkMsg::QueryCurrentConfig;
        assert!(query.config_data().is_none());
    }

    #[test]
    fn test_message_equality() {
        let msg1 = IntentConfigNetworkMsg::config_update(vec!["8.8.8.8".parse().unwrap()], 100);
        let msg2 = IntentConfigNetworkMsg::config_update(vec!["8.8.8.8".parse().unwrap()], 100);
        let msg3 = IntentConfigNetworkMsg::config_update(vec!["1.1.1.1".parse().unwrap()], 100);

        assert_eq!(msg1, msg2);
        assert_ne!(msg1, msg3);
    }

    #[test]
    fn test_query_and_heartbeat() {
        let query = IntentConfigNetworkMsg::QueryCurrentConfig;
        let heartbeat = IntentConfigNetworkMsg::Heartbeat;

        // These should be creatable and comparable
        assert_eq!(query, IntentConfigNetworkMsg::QueryCurrentConfig);
        assert_eq!(heartbeat, IntentConfigNetworkMsg::Heartbeat);
        assert_ne!(query, heartbeat);
    }
}
