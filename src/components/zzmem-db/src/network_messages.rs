//! Network-facing messages for the MemDB component.
use crate::types::PingResult;
use actix::Message as ActixMessage;
use serde::{Deserialize, Serialize};
use zznet_api::{RoomId, PeerId};
use zznet_room::{DeserializationError, RoomMessageTrait, SerializationError};

/// The result of a single ping measurement, as returned by a query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPingResult(pub PingResult);

/// Top-level enum for all network messages related to MemDB.
/// This is the type that will be serialized and sent over the wire.
#[derive(Debug, Clone, Serialize, Deserialize, ActixMessage)]
#[rtype(result = "()")]
pub enum MemDBMessage {
    SubmitBatch {
        peer_id: PeerId,
        timestamp_ms: u64,
        results: Vec<PingResult>,
    },
    BatchAck {
        peer_id: PeerId,
        received_count: usize,
        timestamp_ms: u64,
    },
    Query {
        peer_id: PeerId,
        target: String,
        from_ms: u64,
        to_ms: u64,
    },
    QueryResponse {
        peer_id: PeerId,
        results: Vec<StoredPingResult>,
    },
    HelloCollector {
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

    fn deserialize_for_room(
        room_id: &RoomId,
        bytes: &[u8],
    ) -> Result<Self, DeserializationError> {
        if room_id.as_str() != "memdb" {
            return Err(DeserializationError::UnknownRoom(room_id.clone()));
        }
        rmp_serde::from_slice(bytes).map_err(|e| DeserializationError::MsgPackError(e.to_string()))
    }
}
