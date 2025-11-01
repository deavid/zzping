//! Network protocol messages for Pinger component
//!
//! This module defines the typed messages exchanged between Pinger actors
//! across the network via ZZNet SessionManager. These messages enable
//! remote configuration and monitoring of ping operations.
//!
//! # Protocol Design
//!
//! The protocol supports:
//! - **Collector**: Sends ping configuration updates to Pinger
//! - **Pinger**: Receives configuration and sends ping results to MemDB
//!
//! # Message Flow Examples
//!
//! ## Config Update Flow
//! ```text
//! Collector
//!     ↓ UpdateTargets (to Pinger)
//! SessionManager → Pinger (room: "pinger", 1:1 connection)
//! Pinger → apply new targets to ping operations
//! ```

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use zznet_api::types::RoomId;
use zznet_room::room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError};

/// Network protocol messages for Pinger component
///
/// These messages are exchanged between Collector and Pinger processes
/// via ZZNet SessionManager over the "pinger" room.
#[derive(
    Clone, Debug, Message, Serialize, Deserialize, bincode::Encode, bincode::Decode, PartialEq,
)]
#[rtype(result = "()")]
pub enum PingerMessage {
    /// Configuration update from Collector to Pinger
    ///
    /// Sent when the Collector wants to update the ping targets and rates.
    /// The Pinger receives this and applies it to its ping operations.
    ///
    /// # Flow
    /// Collector → SessionManager → Pinger
    UpdateTargets {
        /// List of targets to ping
        targets: Vec<crate::messages::TargetConfig>,
    },
}

impl RoomMessageTrait for PingerMessage {
    fn room_id(&self) -> RoomId {
        RoomId::from("pinger")
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        bincode::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| SerializationError::BincodeError(e.to_string()))
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        if room_id.as_str() != "pinger" {
            return Err(DeserializationError::UnknownRoom(room_id.clone()));
        }

        bincode::decode_from_slice(bytes, bincode::config::standard())
            .map(|(msg, _)| msg)
            .map_err(|e| DeserializationError::BincodeError(e.to_string()))
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_room_id() {
        let msg = PingerMessage::UpdateTargets { targets: vec![] };
        assert_eq!(msg.room_id(), RoomId::from("pinger"));
    }

    #[test]
    fn test_serialize_deserialize() {
        let original = PingerMessage::UpdateTargets {
            targets: vec![crate::messages::TargetConfig {
                target: "192.168.1.1".to_string(),
                rate_ms: 1000,
                timeout_ms: 5000,
            }],
        };

        let bytes = original.serialize_inner().unwrap();
        let deserialized =
            PingerMessage::deserialize_for_room(&RoomId::from("pinger"), &bytes).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_wrong_room_deserialization() {
        let msg = PingerMessage::UpdateTargets { targets: vec![] };
        let bytes = msg.serialize_inner().unwrap();

        let result = PingerMessage::deserialize_for_room(&RoomId::from("wrong"), &bytes);
        assert!(result.is_err());
    }
}
