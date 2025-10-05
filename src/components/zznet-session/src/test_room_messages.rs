//! Test message enums for validating the application-defined enum architecture
//!
//! This module provides test enums to validate critical design properties:
//! 1. Different applications with different enums can interoperate
//! 2. Room intersection works correctly
//! 3. Type conversions work correctly (TMsg ↔ T)
//! 4. Components never see application enum
//! 5. No double serialization (enum structure not serialized)
//! 6. Graceful degradation (unknown rooms)

use crate::room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError};
use crate::types::RoomId;
use serde::{Deserialize, Serialize};

// ============================================================================
// Mock Room Message Types (Simulating Real Component Messages)
// ============================================================================

/// Mock IntentConfig message (like real IntentConfigMessage)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum IntentConfigMessage {
    ConfigUpdate { targets: Vec<String>, rate: u32 },
    Query,
    Response { config: String },
}

/// Mock MemDB message (like real MemDBMessage)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MemDBMessage {
    Store { key: String, value: String },
    Retrieve { key: String },
    Result { value: Option<String> },
}

/// Mock Health message (like real HealthMessage)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HealthMessage {
    Ping,
    Pong { uptime_seconds: u64 },
}

/// Mock Metrics message (only in some apps)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MetricsMessage {
    ReportLatency { peer: String, latency_ms: u32 },
    GetStats,
}

// ============================================================================
// Application A: Full Collector (All Rooms)
// ============================================================================

/// Application A's message enum: Full collector with all rooms
///
/// This simulates a full-featured collector binary that supports all room types.
#[derive(Clone, Debug, PartialEq)]
pub enum CollectorMessages {
    IntentConfig(IntentConfigMessage),
    MemDB(MemDBMessage),
    Health(HealthMessage),
    Metrics(MetricsMessage),
}

impl RoomMessageTrait for CollectorMessages {
    fn room_id(&self) -> RoomId {
        match self {
            CollectorMessages::IntentConfig(_) => RoomId::from("intentconfig"),
            CollectorMessages::MemDB(_) => RoomId::from("memdb"),
            CollectorMessages::Health(_) => RoomId::from("health"),
            CollectorMessages::Metrics(_) => RoomId::from("metrics"),
        }
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        // CRITICAL: Serialize ONLY the inner message, NOT the enum structure
        let bytes = match self {
            CollectorMessages::IntentConfig(msg) => {
                bincode::serde::encode_to_vec(msg, bincode::config::standard())
            }
            CollectorMessages::MemDB(msg) => {
                bincode::serde::encode_to_vec(msg, bincode::config::standard())
            }
            CollectorMessages::Health(msg) => {
                bincode::serde::encode_to_vec(msg, bincode::config::standard())
            }
            CollectorMessages::Metrics(msg) => {
                bincode::serde::encode_to_vec(msg, bincode::config::standard())
            }
        };
        bytes.map_err(|e| SerializationError::BincodeError(e.to_string()))
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        // CRITICAL: Deserialize based on room name, reconstruct enum wrapper
        match room_id.as_str() {
            "intentconfig" => {
                let msg = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
                    .map(|(value, _)| value)
                    .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
                Ok(CollectorMessages::IntentConfig(msg))
            }
            "memdb" => {
                let msg = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
                    .map(|(value, _)| value)
                    .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
                Ok(CollectorMessages::MemDB(msg))
            }
            "health" => {
                let msg = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
                    .map(|(value, _)| value)
                    .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
                Ok(CollectorMessages::Health(msg))
            }
            "metrics" => {
                let msg = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
                    .map(|(value, _)| value)
                    .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
                Ok(CollectorMessages::Metrics(msg))
            }
            _ => Err(DeserializationError::UnknownRoom(room_id.clone())),
        }
    }

    fn supported_rooms() -> Vec<RoomId> {
        vec![
            RoomId::from("intentconfig"),
            RoomId::from("memdb"),
            RoomId::from("health"),
            RoomId::from("metrics"),
        ]
    }
}

// Conversions: IntentConfigMessage ↔ CollectorMessages
impl From<IntentConfigMessage> for CollectorMessages {
    fn from(msg: IntentConfigMessage) -> Self {
        CollectorMessages::IntentConfig(msg)
    }
}

impl TryFrom<CollectorMessages> for IntentConfigMessage {
    type Error = ();
    fn try_from(msg: CollectorMessages) -> Result<Self, Self::Error> {
        match msg {
            CollectorMessages::IntentConfig(m) => Ok(m),
            _ => Err(()),
        }
    }
}

// Conversions: MemDBMessage ↔ CollectorMessages
impl From<MemDBMessage> for CollectorMessages {
    fn from(msg: MemDBMessage) -> Self {
        CollectorMessages::MemDB(msg)
    }
}

impl TryFrom<CollectorMessages> for MemDBMessage {
    type Error = ();
    fn try_from(msg: CollectorMessages) -> Result<Self, Self::Error> {
        match msg {
            CollectorMessages::MemDB(m) => Ok(m),
            _ => Err(()),
        }
    }
}

// Conversions: HealthMessage ↔ CollectorMessages
impl From<HealthMessage> for CollectorMessages {
    fn from(msg: HealthMessage) -> Self {
        CollectorMessages::Health(msg)
    }
}

impl TryFrom<CollectorMessages> for HealthMessage {
    type Error = ();
    fn try_from(msg: CollectorMessages) -> Result<Self, Self::Error> {
        match msg {
            CollectorMessages::Health(m) => Ok(m),
            _ => Err(()),
        }
    }
}

// Conversions: MetricsMessage ↔ CollectorMessages
impl From<MetricsMessage> for CollectorMessages {
    fn from(msg: MetricsMessage) -> Self {
        CollectorMessages::Metrics(msg)
    }
}

impl TryFrom<CollectorMessages> for MetricsMessage {
    type Error = ();
    fn try_from(msg: CollectorMessages) -> Result<Self, Self::Error> {
        match msg {
            CollectorMessages::Metrics(m) => Ok(m),
            _ => Err(()),
        }
    }
}

// ============================================================================
// Application B: Minimal Client (Subset of Rooms)
// ============================================================================

/// Application B's message enum: Minimal client with only essential rooms
///
/// This simulates a lightweight client that only needs IntentConfig and Health.
/// Notice: No MemDB, No Metrics (different from CollectorMessages!)
#[derive(Clone, Debug, PartialEq)]
pub enum ClientMessages {
    IntentConfig(IntentConfigMessage),
    Health(HealthMessage),
}

impl RoomMessageTrait for ClientMessages {
    fn room_id(&self) -> RoomId {
        match self {
            ClientMessages::IntentConfig(_) => RoomId::from("intentconfig"),
            ClientMessages::Health(_) => RoomId::from("health"),
        }
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        let bytes = match self {
            ClientMessages::IntentConfig(msg) => {
                bincode::serde::encode_to_vec(msg, bincode::config::standard())
            }
            ClientMessages::Health(msg) => {
                bincode::serde::encode_to_vec(msg, bincode::config::standard())
            }
        };
        bytes.map_err(|e| SerializationError::BincodeError(e.to_string()))
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        match room_id.as_str() {
            "intentconfig" => {
                let msg = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
                    .map(|(value, _)| value)
                    .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
                Ok(ClientMessages::IntentConfig(msg))
            }
            "health" => {
                let msg = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
                    .map(|(value, _)| value)
                    .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
                Ok(ClientMessages::Health(msg))
            }
            _ => Err(DeserializationError::UnknownRoom(room_id.clone())),
        }
    }

    fn supported_rooms() -> Vec<RoomId> {
        vec![RoomId::from("intentconfig"), RoomId::from("health")]
    }
}

// Conversions: IntentConfigMessage ↔ ClientMessages
impl From<IntentConfigMessage> for ClientMessages {
    fn from(msg: IntentConfigMessage) -> Self {
        ClientMessages::IntentConfig(msg)
    }
}

impl TryFrom<ClientMessages> for IntentConfigMessage {
    type Error = ();
    fn try_from(msg: ClientMessages) -> Result<Self, Self::Error> {
        match msg {
            ClientMessages::IntentConfig(m) => Ok(m),
            _ => Err(()),
        }
    }
}

// Conversions: HealthMessage ↔ ClientMessages
impl From<HealthMessage> for ClientMessages {
    fn from(msg: HealthMessage) -> Self {
        ClientMessages::Health(msg)
    }
}

impl TryFrom<ClientMessages> for HealthMessage {
    type Error = ();
    fn try_from(msg: ClientMessages) -> Result<Self, Self::Error> {
        match msg {
            ClientMessages::Health(m) => Ok(m),
            _ => Err(()),
        }
    }
}

// ============================================================================
// Helper: Mock Transport for Cross-Enum Testing
// ============================================================================

/// Mock transport that simulates serialization over the wire
///
/// This validates the key property: Different enums can interoperate
/// because only (room_id, inner_bytes) goes over the wire.
#[derive(Debug, Default, Clone)]
pub struct MockTransport {
    /// Queue of messages as they would appear on the wire: (room_id, serialized_bytes)
    pub queue: std::collections::VecDeque<(RoomId, Vec<u8>)>,
}

impl MockTransport {
    /// Send from Application A (CollectorMessages)
    pub fn send_from_collector(
        &mut self,
        msg: CollectorMessages,
    ) -> Result<(), SerializationError> {
        let room_id = msg.room_id();
        let bytes = msg.serialize_inner()?;
        self.queue.push_back((room_id, bytes));
        Ok(())
    }

    /// Receive to Application B (ClientMessages)
    pub fn recv_to_client(&mut self) -> Result<ClientMessages, DeserializationError> {
        let (room_id, bytes) = self
            .queue
            .pop_front()
            .ok_or_else(|| DeserializationError::Custom("Queue empty".to_string()))?;
        ClientMessages::deserialize_for_room(&room_id, &bytes)
    }

    /// Send from Application B (ClientMessages)
    pub fn send_from_client(&mut self, msg: ClientMessages) -> Result<(), SerializationError> {
        let room_id = msg.room_id();
        let bytes = msg.serialize_inner()?;
        self.queue.push_back((room_id, bytes));
        Ok(())
    }

    /// Receive to Application A (CollectorMessages)
    pub fn recv_to_collector(&mut self) -> Result<CollectorMessages, DeserializationError> {
        let (room_id, bytes) = self
            .queue
            .pop_front()
            .ok_or_else(|| DeserializationError::Custom("Queue empty".to_string()))?;
        CollectorMessages::deserialize_for_room(&room_id, &bytes)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // Test 1: Enum Structure Not Serialized (Critical!)
    // ------------------------------------------------------------------------

    #[test]
    fn test_no_double_serialization() {
        // Create a message
        let original = IntentConfigMessage::ConfigUpdate {
            targets: vec!["8.8.8.8".to_string()],
            rate: 100,
        };

        // Serialize via CollectorMessages (with enum wrapper)
        let collector_msg = CollectorMessages::IntentConfig(original.clone());
        let serialized = collector_msg.serialize_inner().unwrap();

        // Serialize directly (without enum wrapper)
        let direct_serialized =
            bincode::serde::encode_to_vec(&original, bincode::config::standard()).unwrap();

        // CRITICAL: They must be identical!
        // This proves the enum structure is NOT serialized.
        assert_eq!(
            serialized, direct_serialized,
            "Enum wrapper MUST NOT add bytes to serialization!"
        );
    }

    // ------------------------------------------------------------------------
    // Test 2: Different Enums Interoperate (via room_id)
    // ------------------------------------------------------------------------

    #[test]
    fn test_cross_enum_intentconfig() {
        let mut transport = MockTransport::default();

        // Collector sends IntentConfig message
        let msg = IntentConfigMessage::Query;
        let collector_msg = CollectorMessages::IntentConfig(msg.clone());
        transport.send_from_collector(collector_msg).unwrap();

        // Client receives it (different enum!)
        let client_msg = transport.recv_to_client().unwrap();

        // Unwrap to concrete type
        let received: IntentConfigMessage = client_msg.try_into().unwrap();
        assert_eq!(received, msg);
    }

    #[test]
    fn test_cross_enum_health() {
        let mut transport = MockTransport::default();

        // Client sends Health message
        let msg = HealthMessage::Ping;
        let client_msg = ClientMessages::Health(msg.clone());
        transport.send_from_client(client_msg).unwrap();

        // Collector receives it (different enum!)
        let collector_msg = transport.recv_to_collector().unwrap();

        // Unwrap to concrete type
        let received: HealthMessage = collector_msg.try_into().unwrap();
        assert_eq!(received, msg);
    }

    // ------------------------------------------------------------------------
    // Test 3: Room Intersection (Unknown Rooms Rejected)
    // ------------------------------------------------------------------------

    #[test]
    fn test_unknown_room_rejected() {
        let mut transport = MockTransport::default();

        // Collector sends MemDB message (not in ClientMessages!)
        let msg = MemDBMessage::Store {
            key: "test".to_string(),
            value: "data".to_string(),
        };
        let collector_msg = CollectorMessages::MemDB(msg);
        transport.send_from_collector(collector_msg).unwrap();

        // Client tries to receive it
        let result = transport.recv_to_client();

        // MUST fail: Client doesn't have MemDB room
        assert!(result.is_err());
        match result {
            Err(DeserializationError::UnknownRoom(room_id)) => {
                assert_eq!(room_id.as_str(), "memdb");
            }
            _ => panic!("Expected UnknownRoom error"),
        }
    }

    #[test]
    fn test_metrics_room_rejected_by_client() {
        let mut transport = MockTransport::default();

        // Collector sends Metrics message
        let msg = MetricsMessage::GetStats;
        let collector_msg = CollectorMessages::Metrics(msg);
        transport.send_from_collector(collector_msg).unwrap();

        // Client tries to receive it
        let result = transport.recv_to_client();

        // MUST fail: Client doesn't have Metrics room
        assert!(result.is_err());
        match result {
            Err(DeserializationError::UnknownRoom(room_id)) => {
                assert_eq!(room_id.as_str(), "metrics");
            }
            _ => panic!("Expected UnknownRoom error"),
        }
    }

    // ------------------------------------------------------------------------
    // Test 4: supported_rooms() Lists Rooms Correctly
    // ------------------------------------------------------------------------

    #[test]
    fn test_supported_rooms_collector() {
        let rooms = CollectorMessages::supported_rooms();
        assert_eq!(rooms.len(), 4);
        assert!(rooms.contains(&RoomId::from("intentconfig")));
        assert!(rooms.contains(&RoomId::from("memdb")));
        assert!(rooms.contains(&RoomId::from("health")));
        assert!(rooms.contains(&RoomId::from("metrics")));
    }

    #[test]
    fn test_supported_rooms_client() {
        let rooms = ClientMessages::supported_rooms();
        assert_eq!(rooms.len(), 2);
        assert!(rooms.contains(&RoomId::from("intentconfig")));
        assert!(rooms.contains(&RoomId::from("health")));
        // NOT memdb, NOT metrics
        assert!(!rooms.contains(&RoomId::from("memdb")));
        assert!(!rooms.contains(&RoomId::from("metrics")));
    }

    #[test]
    fn test_room_intersection() {
        let collector_rooms = CollectorMessages::supported_rooms();
        let client_rooms = ClientMessages::supported_rooms();

        // Compute intersection
        let intersection: Vec<_> = collector_rooms
            .iter()
            .filter(|r| client_rooms.contains(r))
            .cloned()
            .collect();

        // Should be [intentconfig, health]
        assert_eq!(intersection.len(), 2);
        assert!(intersection.contains(&RoomId::from("intentconfig")));
        assert!(intersection.contains(&RoomId::from("health")));
    }

    // ------------------------------------------------------------------------
    // Test 5: Type Conversions Work Correctly
    // ------------------------------------------------------------------------

    #[test]
    fn test_from_conversion() {
        let msg = IntentConfigMessage::Query;

        // IntentConfigMessage → CollectorMessages
        let collector_msg: CollectorMessages = msg.clone().into();
        assert_eq!(collector_msg.room_id().as_str(), "intentconfig");

        // IntentConfigMessage → ClientMessages
        let client_msg: ClientMessages = msg.into();
        assert_eq!(client_msg.room_id().as_str(), "intentconfig");
    }

    #[test]
    fn test_try_from_success() {
        let collector_msg = CollectorMessages::IntentConfig(IntentConfigMessage::Query);

        // CollectorMessages → IntentConfigMessage (success)
        let result: Result<IntentConfigMessage, ()> = collector_msg.try_into();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), IntentConfigMessage::Query);
    }

    #[test]
    fn test_try_from_failure() {
        let collector_msg = CollectorMessages::MemDB(MemDBMessage::Retrieve {
            key: "test".to_string(),
        });

        // CollectorMessages → IntentConfigMessage (failure: wrong variant)
        let result: Result<IntentConfigMessage, ()> = collector_msg.try_into();
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------------
    // Test 6: room_id() Extraction Works
    // ------------------------------------------------------------------------

    #[test]
    fn test_room_id_extraction() {
        let intentconfig = CollectorMessages::IntentConfig(IntentConfigMessage::Query);
        assert_eq!(intentconfig.room_id().as_str(), "intentconfig");

        let memdb = CollectorMessages::MemDB(MemDBMessage::Retrieve {
            key: "test".to_string(),
        });
        assert_eq!(memdb.room_id().as_str(), "memdb");

        let health = CollectorMessages::Health(HealthMessage::Ping);
        assert_eq!(health.room_id().as_str(), "health");

        let metrics = CollectorMessages::Metrics(MetricsMessage::GetStats);
        assert_eq!(metrics.room_id().as_str(), "metrics");
    }

    // ------------------------------------------------------------------------
    // Test 7: Round-Trip Serialization
    // ------------------------------------------------------------------------

    #[test]
    fn test_roundtrip_intentconfig() {
        let original = IntentConfigMessage::ConfigUpdate {
            targets: vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()],
            rate: 50,
        };

        // Wrap in enum
        let collector_msg = CollectorMessages::IntentConfig(original.clone());

        // Serialize
        let room_id = collector_msg.room_id();
        let bytes = collector_msg.serialize_inner().unwrap();

        // Deserialize
        let deserialized = CollectorMessages::deserialize_for_room(&room_id, &bytes).unwrap();

        // Unwrap
        let result: IntentConfigMessage = deserialized.try_into().unwrap();
        assert_eq!(result, original);
    }

    #[test]
    fn test_roundtrip_memdb() {
        let original = MemDBMessage::Store {
            key: "mykey".to_string(),
            value: "myvalue".to_string(),
        };

        let collector_msg = CollectorMessages::MemDB(original.clone());
        let room_id = collector_msg.room_id();
        let bytes = collector_msg.serialize_inner().unwrap();
        let deserialized = CollectorMessages::deserialize_for_room(&room_id, &bytes).unwrap();
        let result: MemDBMessage = deserialized.try_into().unwrap();
        assert_eq!(result, original);
    }

    #[test]
    fn test_roundtrip_health() {
        let original = HealthMessage::Pong {
            uptime_seconds: 3600,
        };

        // Test with ClientMessages
        let client_msg = ClientMessages::Health(original.clone());
        let room_id = client_msg.room_id();
        let bytes = client_msg.serialize_inner().unwrap();
        let deserialized = ClientMessages::deserialize_for_room(&room_id, &bytes).unwrap();
        let result: HealthMessage = deserialized.try_into().unwrap();
        assert_eq!(result, original);
    }

    // ------------------------------------------------------------------------
    // Test 8: Complete End-to-End Flow
    // ------------------------------------------------------------------------

    #[test]
    fn test_complete_cross_app_flow() {
        // Simulates: Collector ↔ Client communication over transport
        let mut transport = MockTransport::default();

        // Phase 1: Collector sends IntentConfig query
        let query = IntentConfigMessage::Query;
        let collector_msg = CollectorMessages::IntentConfig(query);
        transport.send_from_collector(collector_msg).unwrap();

        // Phase 2: Client receives query (different enum!)
        let client_received = transport.recv_to_client().unwrap();
        let client_query: IntentConfigMessage = client_received.try_into().unwrap();
        assert_eq!(client_query, IntentConfigMessage::Query);

        // Phase 3: Client responds with config
        let response = IntentConfigMessage::Response {
            config: "rate=100".to_string(),
        };
        let client_response = ClientMessages::IntentConfig(response.clone());
        transport.send_from_client(client_response).unwrap();

        // Phase 4: Collector receives response
        let collector_received = transport.recv_to_collector().unwrap();
        let collector_response: IntentConfigMessage = collector_received.try_into().unwrap();
        assert_eq!(collector_response, response);

        // Phase 5: Collector sends Health ping
        let ping = HealthMessage::Ping;
        let collector_ping = CollectorMessages::Health(ping);
        transport.send_from_collector(collector_ping).unwrap();

        // Phase 6: Client receives ping, responds with pong
        let client_ping_received = transport.recv_to_client().unwrap();
        let client_ping: HealthMessage = client_ping_received.try_into().unwrap();
        assert_eq!(client_ping, HealthMessage::Ping);

        let pong = HealthMessage::Pong {
            uptime_seconds: 3600,
        };
        let client_pong = ClientMessages::Health(pong.clone());
        transport.send_from_client(client_pong).unwrap();

        // Phase 7: Collector receives pong
        let collector_pong_received = transport.recv_to_collector().unwrap();
        let collector_pong: HealthMessage = collector_pong_received.try_into().unwrap();
        assert_eq!(collector_pong, pong);

        // Phase 8: Collector tries to send MemDB message (should fail on client side)
        let memdb = MemDBMessage::Store {
            key: "test".to_string(),
            value: "data".to_string(),
        };
        let collector_memdb = CollectorMessages::MemDB(memdb);
        transport.send_from_collector(collector_memdb).unwrap();

        // Phase 9: Client rejects MemDB (not in its enum)
        let result = transport.recv_to_client();
        assert!(result.is_err());
        match result {
            Err(DeserializationError::UnknownRoom(room_id)) => {
                assert_eq!(room_id.as_str(), "memdb");
            }
            _ => panic!("Expected UnknownRoom error for memdb"),
        }

        // SUCCESS: Complete bidirectional flow with different enums works!
        // Only shared rooms (intentconfig, health) succeed
        // Unknown rooms (memdb) are properly rejected
    }

    #[test]
    fn test_serialization_size_efficiency() {
        // Verify that enum wrapper adds ZERO bytes
        let msg = IntentConfigMessage::ConfigUpdate {
            targets: vec!["8.8.8.8".to_string()],
            rate: 100,
        };

        // Direct serialization
        let direct_size = bincode::serde::encode_to_vec(&msg, bincode::config::standard())
            .unwrap()
            .len();

        // Via CollectorMessages
        let collector_msg = CollectorMessages::IntentConfig(msg.clone());
        let collector_size = collector_msg.serialize_inner().unwrap().len();

        // Via ClientMessages
        let client_msg = ClientMessages::IntentConfig(msg);
        let client_size = client_msg.serialize_inner().unwrap().len();

        // All must be identical size
        assert_eq!(direct_size, collector_size);
        assert_eq!(direct_size, client_size);

        // This proves zero overhead from enum wrapper
        println!(
            "Message size: {} bytes (no overhead from enum wrapper)",
            direct_size
        );
    }

    #[test]
    fn test_all_collector_rooms_listed() {
        // Ensure all rooms in CollectorMessages are listed
        let rooms = CollectorMessages::supported_rooms();

        // Must have exactly 4 rooms
        assert_eq!(rooms.len(), 4, "CollectorMessages should have 4 rooms");

        // Check each room is present
        let room_names: Vec<String> = rooms.iter().map(|r| r.to_string()).collect();
        assert!(
            room_names.contains(&"intentconfig".to_string()),
            "Missing intentconfig"
        );
        assert!(room_names.contains(&"memdb".to_string()), "Missing memdb");
        assert!(room_names.contains(&"health".to_string()), "Missing health");
        assert!(
            room_names.contains(&"metrics".to_string()),
            "Missing metrics"
        );
    }

    #[test]
    fn test_all_client_rooms_listed() {
        // Ensure all rooms in ClientMessages are listed
        let rooms = ClientMessages::supported_rooms();

        // Must have exactly 2 rooms
        assert_eq!(rooms.len(), 2, "ClientMessages should have 2 rooms");

        // Check each room is present
        let room_names: Vec<String> = rooms.iter().map(|r| r.to_string()).collect();
        assert!(
            room_names.contains(&"intentconfig".to_string()),
            "Missing intentconfig"
        );
        assert!(room_names.contains(&"health".to_string()), "Missing health");

        // Should NOT have memdb or metrics
        assert!(
            !room_names.contains(&"memdb".to_string()),
            "Client shouldn't have memdb"
        );
        assert!(
            !room_names.contains(&"metrics".to_string()),
            "Client shouldn't have metrics"
        );
    }
}
