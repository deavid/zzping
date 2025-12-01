//! Network messages for the MemDB component.
//!
//! These messages are sent over the network via ZZNet rooms.
//! They are serialized/deserialized at the transport boundary.

use crate::types::PingResult;
use actix::prelude::*;
use serde::{Deserialize, Serialize};
use zznet_api::RoomId;
use zznet_room::{DeserializationError, RoomMessageTrait, SerializationError};

/// Messages exchanged between MemDB components over the network.
#[derive(Serialize, Deserialize, Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum MemDBMessage {
    /// Collector → Database: Submit a batch of ping results
    SubmitBatch {
        /// The peer ID of the sender (filled by the routing layer)
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
        /// The peer ID of the sender (filled by the routing layer)
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

/// A stored ping result in the database.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StoredPingResult {
    /// Target host
    pub target: String,
    /// Timestamp when ping was sent
    pub timestamp_ms: u64,
    /// Round-trip time in microseconds (None if packet lost)
    pub rtt_us: Option<u32>,
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
        // For now, use MessagePack for serialization
        // In production, this might use a more efficient format
        rmp_serde::to_vec(self).map_err(|e| SerializationError::MsgPackError(e.to_string()))
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        // For now, all messages go to the same room
        if room_id.as_str() != "memdb" {
            return Err(DeserializationError::UnknownRoom(room_id.clone()));
        }

        rmp_serde::from_slice(bytes).map_err(|e| DeserializationError::MsgPackError(e.to_string()))
    }
}
