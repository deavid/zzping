//! Network protocol messages for IntentConfig component
//!
//! This module defines the typed messages exchanged between IntentConfig actors
//! across the network via ZZNet SessionManager. These messages enable bidirectional
//! communication for configuration management.
//!
//! # Protocol Design
//!
//! The protocol supports three roles:
//! - **Database**: Authoritative source, sends config updates
//! - **Collector**: Passive receiver, accepts config updates
//! - **AdminClient**: Operator, requests config changes
//!
//! # Message Flow Examples
//!
//! ## Config Update Flow (Normal Operation)
//! ```text
//! AdminClient
//!     ↓ RequestConfigChange (to Database)
//! Database
//!     ↓ persist to disk
//!     ↓ send ConfigUpdate individually to each Collector
//! SessionManager → Collector-1 (room: "intent-config", 1:1 connection)
//! SessionManager → Collector-2 (room: "intent-config", 1:1 connection)
//! SessionManager → Collector-N (room: "intent-config", 1:1 connection)
//! Each Collector → apply to local operations
//! ```
//!
//! **Architecture Note:** Each Database-Collector connection has its own
//! dedicated "intent-config" room. Rooms are 1:1 point-to-point channels,
//! NOT broadcast channels. Database sends ConfigUpdate individually to each
//! connected Collector peer.
//!
//! ## Query Flow (Database startup/recovery)
//! ```text
//! Database reads from disk on startup
//!     ↓ sends ConfigUpdate to each connected Collector (1:1)
//! Collectors receive initial state
//! ```

use actix::prelude::*;
use log;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use zznet_api::types::RoomId;
use zznet_room::room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError};

/// Network protocol messages for IntentConfig component
///
/// These messages are exchanged between Collector and Database processes
/// via ZZNet SessionManager over the "intentconfig" room.
///
/// All messages are strongly typed and transport-agnostic - they never
/// touch bytes or serialization at this layer.
#[derive(Clone, Debug, Message, Serialize, Deserialize, PartialEq)]
#[rtype(result = "()")]
pub enum IntentConfigNetworkMsg {
    /// Administrative request to change configuration
    ///
    /// Sent by AdminClient to Database to request a configuration change.
    /// Database validates, persists, and sends individually to each Collector.
    ///
    /// **Security:** This message should only be accepted from peers with
    /// ClientAdmin role. The Database actor MUST check the sender's role
    /// before applying changes.
    ///
    /// **Sender Context:** The `sender_peer_id` field is filled by the
    /// SessionManager and represents the peer ID of the connection that
    /// sent this message. The actor can use this to query the peer's role
    /// via `session_manager.get_peer_role(&sender_peer_id)`.
    ///
    /// # Fields
    /// * `sender_peer_id` - The peer ID of the requester (for role lookup)
    /// * `targets` - List of IP addresses to ping
    /// * `ping_rate_pps` - Ping rate in packets per second
    RequestConfigChange {
        /// The peer ID of the requester (filled by SessionManager)
        sender_peer_id: String,
        /// List of IP addresses to ping
        targets: Vec<IpAddr>,
        /// Ping rate in packets per second
        ping_rate_pps: u64,
    },

    /// Configuration update from Database to Collectors
    ///
    /// Sent whenever the Database changes configuration (via AdminClient request
    /// or on startup). Collectors receive this and apply it to their operations.
    ///
    /// # Flow
    /// Database → SessionManager → Collector(s)
    ConfigUpdate {
        /// List of IP addresses to ping
        targets: Vec<IpAddr>,
        /// Ping rate in packets per second
        ping_rate_pps: u64,
    },

    /// Request for current configuration from Database to Collector
    ///
    /// Sent by AdminClient to query current configuration, or by Database to Collector
    /// during recovery scenarios. Database responds with CurrentConfig message.
    ///
    /// # Flow
    /// AdminClient/Database → SessionManager → Database (responds with CurrentConfig)
    QueryCurrentConfig,

    /// Response with current configuration from Collector to Database
    ///
    /// Sent by Database in response to QueryCurrentConfig, containing the current
    /// configuration state for recovery or display purposes.
    ///
    /// # Flow
    /// Database → SessionManager → All peers (broadcast response)
    CurrentConfig {
        /// List of IP addresses to ping
        targets: Vec<IpAddr>,
        /// Ping rate in packets per second
        ping_rate_pps: u64,
    },

    /// Heartbeat message
    ///
    /// Periodic keepalive message to detect connection failures and
    /// differentiate between network partition vs peer crash.
    /// Both Database and Collector roles accept heartbeat messages.
    Heartbeat,

    /// Error response
    ///
    /// Sent by Database to requesters for authorization failures, persistence errors,
    /// or other issues. Both Database and Collector roles can receive error messages
    /// and log them appropriately.
    Error {
        /// Human-readable error description
        reason: String,
    },
}

impl IntentConfigNetworkMsg {
    /// Creates ConfigUpdate message for Database to send to Collectors.
    ///
    /// Use this for internal distribution after persisting configuration.
    /// For admin requests, use RequestConfigChange instead.
    pub fn config_update(targets: Vec<IpAddr>, ping_rate_pps: u64) -> Self {
        Self::ConfigUpdate {
            targets,
            ping_rate_pps,
        }
    }

    /// Helper to create a CurrentConfig message
    pub fn current_config(targets: Vec<IpAddr>, ping_rate_pps: u64) -> Self {
        Self::CurrentConfig {
            targets,
            ping_rate_pps,
        }
    }

    /// Helper to create an Error message
    pub fn error(reason: impl Into<String>) -> Self {
        Self::Error {
            reason: reason.into(),
        }
    }

    /// Identifies messages that carry configuration data (ConfigUpdate, CurrentConfig).
    ///
    /// Useful for filtering or routing messages based on whether they affect
    /// configuration state.
    pub fn has_config_data(&self) -> bool {
        matches!(self, Self::ConfigUpdate { .. } | Self::CurrentConfig { .. })
    }

    /// Extract config data if this is a ConfigUpdate or CurrentConfig message
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
        // All intent config messages use the same room for now
        // In the future, this could be based on collector hostname or other criteria
        RoomId::from("intent-config")
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        // For now, use MessagePack for serialization
        // In production, this might use a more efficient format
        log::debug!("[INTENT-CONFIG SEND] Serializing message: {:?}", self);
        let result =
            rmp_serde::to_vec(self).map_err(|e| SerializationError::MsgPackError(e.to_string()));
        if let Ok(ref bytes) = result {
            log::debug!("[INTENT-CONFIG SEND] Serialized to {} bytes", bytes.len());
        }
        result
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        // For now, all messages go to the same room
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
