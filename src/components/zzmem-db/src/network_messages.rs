//! Network-facing messages for the MemDB component.
use crate::types::PingResult;
use actix::Message as ActixMessage;
use serde::{Deserialize, Serialize};
use zznet_api::{PeerId, RoomId};
use zznet_room::{DeserializationError, RoomMessageTrait, SerializationError};

/// The result of a single ping measurement, as returned by a query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPingResult(pub PingResult);

/// Top-level enum for all network messages related to MemDB.
/// This is the type that will be serialized and sent over the wire.
#[derive(Debug, Clone, Serialize, Deserialize, ActixMessage)]
#[rtype(result = "()")]
pub enum MemDBMessage {
    /// A batch of ping results submitted by a collector.
    SubmitBatch {
        /// The ID of the peer submitting the batch.
        peer_id: PeerId,
        /// The timestamp when the batch was created/sent (ms since epoch).
        timestamp_ms: u64,
        /// The list of ping results in the batch.
        results: Vec<PingResult>,
    },
    /// Acknowledgment that a batch was received and processed.
    BatchAck {
        /// The ID of the peer acknowledging the batch.
        peer_id: PeerId,
        /// The number of results received in the batch.
        received_count: usize,
        /// The timestamp of the batch being acknowledged.
        timestamp_ms: u64,
    },
    /// A query for ping results.
    Query {
        /// The ID of the peer making the query.
        peer_id: PeerId,
        /// The target to query for (e.g., IP address or hostname).
        target: String,
        /// The start timestamp for the query range (ms since epoch).
        from_ms: u64,
        /// The end timestamp for the query range (ms since epoch).
        to_ms: u64,
    },
    /// The response to a query.
    QueryResponse {
        /// The ID of the peer responding to the query.
        peer_id: PeerId,
        /// The list of results matching the query.
        results: Vec<StoredPingResult>,
    },
    /// A message sent by the database to a collector upon connection,
    /// indicating the last timestamp that has been persisted.
    /// This allows the collector to replay any data that might have been lost.
    HelloCollector {
        /// The timestamp of the last persisted result (ms since epoch).
        last_persisted_ts: u64,
    },
}

impl RoomMessageTrait for MemDBMessage {
    fn room_id(&self) -> RoomId {
        RoomId::new("memdb")
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        rmp_serde::to_vec(self).map_err(|e| SerializationError::MsgPackError(e.to_string()))
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        if room_id.as_str() != "memdb" {
            return Err(DeserializationError::UnknownRoom(room_id.clone()));
        }
        rmp_serde::from_slice(bytes).map_err(|e| DeserializationError::MsgPackError(e.to_string()))
    }
}
