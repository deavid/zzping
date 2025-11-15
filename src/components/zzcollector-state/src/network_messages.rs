//! Defines the network messages for the `zzcollector-state` component.
//!
//! These messages are exchanged between collector and database roles over the network.

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use zznet_api::types::RoomId;
use zznet_room::room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError};

/// The name of the room used for collector state communication.
pub const CSTATE_ROOM: &str = "cstate";

/// Enum representing all possible messages for the collector state room.
#[derive(
    Clone, Debug, Message, Serialize, Deserialize, bincode::Encode, bincode::Decode, PartialEq,
)]
#[rtype(result = "()")]
pub enum CStateMessage {
    /// Collector -> Database: Register and report health.
    Heartbeat {
        /// The unique ID of the collector.
        collector_id: String,
        /// The uptime of the collector in seconds.
        uptime_secs: u64,
        /// The total number of pings sent by the collector.
        pings_sent: u64,
        /// The total number of pings received by the collector.
        pings_received: u64,
        /// The total number of batches sent by the collector.
        batches_sent: u64,
        /// The timestamp of the last configuration update.
        last_config_update_ms: u64,
        /// A unique nonce for the collector's connection.
        connection_nonce: u64,
    },

    /// Database -> Collector: Acknowledgment with server time.
    HeartbeatAck {
        /// The timestamp of the heartbeat being acknowledged.
        timestamp_ms: u64,
        /// The server's time.
        server_time_ms: u64,
    },

    /// Database -> Admin: List of active collectors (admin only).
    CollectorList {
        /// A list of active collectors.
        collectors: Vec<CollectorInfo>,
    },

    /// Database -> Collector: Indicates registration/rejection when database is at capacity
    RegistrationRejected {
        /// Human-readable reason for rejection
        reason: String,
    },

    /// Database -> Peer: Indicates requester is unauthorized to perform the action
    Unauthorized {
        /// Human-readable reason for denial
        reason: String,
    },

    /// Admin -> Database: Request collector list.
    QueryCollectors,
}

/// Information about a single collector, used in `CollectorList`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, bincode::Encode, bincode::Decode)]
pub struct CollectorInfo {
    /// The unique ID of the collector.
    pub id: String,
    /// The timestamp of the last heartbeat received from the collector.
    pub last_seen_ms: u64,
    /// The uptime of the collector in seconds.
    pub uptime_secs: u64,
    /// The total number of pings sent by the collector.
    pub pings_sent: u64,
    /// The total number of pings received by the collector.
    pub pings_received: u64,
    /// The connection nonce of the collector.
    pub connection_nonce: u64,
}

impl RoomMessageTrait for CStateMessage {
    fn room_id(&self) -> RoomId {
        RoomId::from(CSTATE_ROOM)
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        ron::to_string(self)
            .map(|s| s.into_bytes())
            .map_err(|e| SerializationError::Failed(e.to_string()))
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        if room_id.as_str() != CSTATE_ROOM {
            return Err(DeserializationError::UnknownRoom(room_id.clone()));
        }
        ron::de::from_bytes(bytes).map_err(|e| DeserializationError::Failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that a `Heartbeat` message can be serialized and deserialized correctly.
    #[test]
    fn test_heartbeat_serialization_deserialization() {
        let original_message = CStateMessage::Heartbeat {
            collector_id: "collector-1".to_string(),
            uptime_secs: 12345,
            pings_sent: 100,
            pings_received: 95,
            batches_sent: 10,
            last_config_update_ms: 987654321,
            connection_nonce: 1122334455,
        };
        let room_id = original_message.room_id();
        let bytes = original_message.serialize_inner().unwrap();
        let deserialized = CStateMessage::deserialize_for_room(&room_id, &bytes).unwrap();

        assert!(matches!(deserialized, CStateMessage::Heartbeat { .. }));
        if let CStateMessage::Heartbeat { collector_id, .. } = deserialized {
            assert_eq!(collector_id, "collector-1");
        }
    }
}
