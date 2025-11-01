//! Network messages for the MemDB component.
//!
//! These messages are sent over the network via ZZNet rooms.
//! They are serialized/deserialized at the transport boundary.

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use zznet_api::types::RoomId;
use zznet_room::room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError};

/// Messages exchanged between MemDB components over the network.
#[derive(Serialize, Deserialize, Debug, Clone, bincode::Encode, bincode::Decode, Message)]
#[rtype(result = "()")]
pub enum MemDBMessage {
    /// Collector → Database: Submit a batch of ping results
    SubmitBatch {
        /// The peer ID of the sender (filled by SessionManager)
        sender_peer_id: String,
        /// Timestamp when the batch was created (milliseconds since epoch)
        timestamp_ms: u64,
        /// The ping results in this batch
        results: Vec<PingResult>,
    },

    /// Database → Collector: Acknowledge receipt of batch
    BatchAck {
        /// Number of results received
        received_count: usize,
        /// Timestamp when the batch was acknowledged
        timestamp_ms: u64,
    },

    /// Admin → Database: Query stored ping data
    Query {
        /// The peer ID of the sender (filled by SessionManager)
        sender_peer_id: String,
        /// Target host to query
        target: String,
        /// Start time for query (milliseconds since epoch)
        from_ms: u64,
        /// End time for query (milliseconds since epoch)
        to_ms: u64,
    },

    /// Database → Admin: Response to query
    QueryResponse {
        /// The query results
        results: Vec<StoredPingResult>,
    },
}

/// A single ping result from the collector.
#[derive(Serialize, Deserialize, Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct PingResult {
    /// Target host that was pinged
    pub target: String,
    /// Timestamp when ping was sent (milliseconds since epoch)
    pub timestamp_ms: u64,
    /// Round-trip time in microseconds (None if packet lost)
    pub rtt_us: Option<u32>,
    /// Sequence number for this ping
    pub sequence: u32,
}

/// A stored ping result in the database.
#[derive(Serialize, Deserialize, Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct StoredPingResult {
    /// Target host
    pub target: String,
    /// Timestamp when ping was sent
    pub timestamp_ms: u64,
    /// Round-trip time in microseconds (None if packet lost)
    pub rtt_us: Option<u32>,
    /// Sequence number
    pub sequence: u32,
    /// When this result was stored in database
    pub stored_at_ms: u64,
}

impl RoomMessageTrait for MemDBMessage {
    fn room_id(&self) -> RoomId {
        // All memdb messages use the same room for now
        // In the future, this could be based on collector hostname or other criteria
        RoomId::from("memdb")
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        // For now, use bincode for serialization
        // In production, this might use a more efficient format
        bincode::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| SerializationError::BincodeError(e.to_string()))
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        // For now, all messages go to the same room
        if room_id.as_str() != "memdb" {
            return Err(DeserializationError::UnknownRoom(room_id.clone()));
        }

        bincode::decode_from_slice(bytes, bincode::config::standard())
            .map(|(value, _)| value)
            .map_err(|e| DeserializationError::BincodeError(e.to_string()))
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ping_result_serialization() {
        let result = PingResult {
            target: "8.8.8.8".to_string(),
            timestamp_ms: 1234567890,
            rtt_us: Some(15000),
            sequence: 42,
        };

        let serialized = bincode::encode_to_vec(&result, bincode::config::standard()).unwrap();
        let deserialized: PingResult =
            bincode::decode_from_slice(&serialized, bincode::config::standard())
                .unwrap()
                .0;

        assert_eq!(result.target, deserialized.target);
        assert_eq!(result.timestamp_ms, deserialized.timestamp_ms);
        assert_eq!(result.rtt_us, deserialized.rtt_us);
        assert_eq!(result.sequence, deserialized.sequence);
    }

    #[test]
    fn test_memdb_message_serialization() {
        let message = MemDBMessage::SubmitBatch {
            sender_peer_id: "peer-123".to_string(),
            timestamp_ms: 1234567890,
            results: vec![PingResult {
                target: "8.8.8.8".to_string(),
                timestamp_ms: 1234567890,
                rtt_us: Some(15000),
                sequence: 42,
            }],
        };

        let serialized = bincode::encode_to_vec(&message, bincode::config::standard()).unwrap();
        let deserialized: MemDBMessage =
            bincode::decode_from_slice(&serialized, bincode::config::standard())
                .unwrap()
                .0;

        match deserialized {
            MemDBMessage::SubmitBatch {
                sender_peer_id,
                timestamp_ms,
                results,
            } => {
                assert_eq!(sender_peer_id, "peer-123");
                assert_eq!(timestamp_ms, 1234567890);
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].target, "8.8.8.8");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_room_id() {
        let message = MemDBMessage::SubmitBatch {
            sender_peer_id: "peer-123".to_string(),
            timestamp_ms: 1234567890,
            results: vec![],
        };
        assert_eq!(message.room_id(), RoomId::from("memdb"));
    }

    #[test]
    fn test_serialize_inner() {
        let message = MemDBMessage::BatchAck {
            received_count: 5,
            timestamp_ms: 1234567890,
        };

        let result = message.serialize_inner();
        assert!(result.is_ok());

        let bytes = result.unwrap();
        assert!(!bytes.is_empty());

        // Verify it can be deserialized back
        let deserialized = MemDBMessage::deserialize_for_room(&RoomId::from("memdb"), &bytes);
        assert!(deserialized.is_ok());
        assert!(matches!(
            deserialized.unwrap(),
            MemDBMessage::BatchAck { .. }
        ));
    }

    #[test]
    fn test_deserialize_for_room_valid() {
        let message = MemDBMessage::Query {
            sender_peer_id: "peer-123".to_string(),
            target: "example.com".to_string(),
            from_ms: 1000,
            to_ms: 2000,
        };

        let bytes = message.serialize_inner().unwrap();
        let result = MemDBMessage::deserialize_for_room(&RoomId::from("memdb"), &bytes);

        assert!(result.is_ok());
        match result.unwrap() {
            MemDBMessage::Query {
                sender_peer_id,
                target,
                from_ms,
                to_ms,
            } => {
                assert_eq!(sender_peer_id, "peer-123");
                assert_eq!(target, "example.com");
                assert_eq!(from_ms, 1000);
                assert_eq!(to_ms, 2000);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_deserialize_for_room_wrong_room() {
        let message = MemDBMessage::QueryResponse { results: vec![] };
        let bytes = message.serialize_inner().unwrap();

        let result = MemDBMessage::deserialize_for_room(&RoomId::from("wrong-room"), &bytes);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DeserializationError::UnknownRoom(_)
        ));
    }

    #[test]
    fn test_deserialize_for_room_invalid_data() {
        // Use empty data which should definitely fail to decode
        let invalid_bytes = vec![]; // Empty data
        let result = MemDBMessage::deserialize_for_room(&RoomId::from("memdb"), &invalid_bytes);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DeserializationError::BincodeError(_)
        ));
    }

}
