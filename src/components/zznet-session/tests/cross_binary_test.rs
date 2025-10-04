//! Cross-Binary Integration Test
//!
//! This test validates the core architectural principle: different applications
//! with different enum structures can communicate through the same network layer.
//!
//! Test Setup:
//! - Binary A (Collector): CollectorMessages enum with 4 room variants
//! - Binary B (Database): DatabaseMessages enum with 3 room variants (no Metrics)
//! - Common rooms: intentconfig, memdb, health
//! - Uncommon room: metrics (only in Collector)
//!
//! What This Proves:
//! 1. Different enums can coexist (CollectorMessages ≠ DatabaseMessages)
//! 2. Enum structure never serialized (only inner messages)
//! 3. Wire format is compatible (same inner message, different enum wrappers)
//! 4. supported_rooms() correctly declares different room sets
//! 5. Messages serialized by one enum can be deserialized by another

use serde::{Deserialize, Serialize};
use zznet_session::room_message_trait::{
    DeserializationError, RoomMessageTrait, SerializationError,
};
use zznet_session::types::RoomId;

// ============================================================================
// Shared Room Message Types (Common to Both Binaries)
// ============================================================================

/// IntentConfig room messages
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum IntentConfigMessage {
    ConfigUpdate { targets: Vec<String>, rate: u32 },
    Query,
    Response { config: String },
}

/// MemDB room messages
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MemDBMessage {
    Store { key: String, value: String },
    Retrieve { key: String },
    Result { value: Option<String> },
}

/// Health room messages
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HealthMessage {
    Ping,
    Pong { uptime_seconds: u64 },
}

/// Metrics room messages (only in Collector)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MetricsMessage {
    ReportLatency { peer: String, latency_ms: u32 },
    GetStats,
}

// ============================================================================
// Binary A: Collector (4 Rooms)
// ============================================================================

/// Collector binary's application-defined enum
#[derive(Clone, Debug, PartialEq)]
pub enum CollectorMessages {
    IntentConfig(IntentConfigMessage),
    MemDB(MemDBMessage),
    Health(HealthMessage),
    Metrics(MetricsMessage), // ← Only in Collector
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
        let bytes = match self {
            CollectorMessages::IntentConfig(msg) => bincode::serialize(msg),
            CollectorMessages::MemDB(msg) => bincode::serialize(msg),
            CollectorMessages::Health(msg) => bincode::serialize(msg),
            CollectorMessages::Metrics(msg) => bincode::serialize(msg),
        };
        bytes.map_err(|e| SerializationError::BincodeError(e.to_string()))
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        match room_id.as_str() {
            "intentconfig" => {
                let msg = bincode::deserialize::<IntentConfigMessage>(bytes)
                    .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
                Ok(CollectorMessages::IntentConfig(msg))
            }
            "memdb" => {
                let msg = bincode::deserialize::<MemDBMessage>(bytes)
                    .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
                Ok(CollectorMessages::MemDB(msg))
            }
            "health" => {
                let msg = bincode::deserialize::<HealthMessage>(bytes)
                    .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
                Ok(CollectorMessages::Health(msg))
            }
            "metrics" => {
                let msg = bincode::deserialize::<MetricsMessage>(bytes)
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

// ============================================================================
// Binary B: Database (3 Rooms - No Metrics!)
// ============================================================================

/// Database binary's application-defined enum
#[derive(Clone, Debug, PartialEq)]
pub enum DatabaseMessages {
    IntentConfig(IntentConfigMessage),
    MemDB(MemDBMessage),
    Health(HealthMessage),
    // NO Metrics variant!
}

impl RoomMessageTrait for DatabaseMessages {
    fn room_id(&self) -> RoomId {
        match self {
            DatabaseMessages::IntentConfig(_) => RoomId::from("intentconfig"),
            DatabaseMessages::MemDB(_) => RoomId::from("memdb"),
            DatabaseMessages::Health(_) => RoomId::from("health"),
        }
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        let bytes = match self {
            DatabaseMessages::IntentConfig(msg) => bincode::serialize(msg),
            DatabaseMessages::MemDB(msg) => bincode::serialize(msg),
            DatabaseMessages::Health(msg) => bincode::serialize(msg),
        };
        bytes.map_err(|e| SerializationError::BincodeError(e.to_string()))
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        match room_id.as_str() {
            "intentconfig" => {
                let msg = bincode::deserialize::<IntentConfigMessage>(bytes)
                    .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
                Ok(DatabaseMessages::IntentConfig(msg))
            }
            "memdb" => {
                let msg = bincode::deserialize::<MemDBMessage>(bytes)
                    .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
                Ok(DatabaseMessages::MemDB(msg))
            }
            "health" => {
                let msg = bincode::deserialize::<HealthMessage>(bytes)
                    .map_err(|e| DeserializationError::BincodeError(e.to_string()))?;
                Ok(DatabaseMessages::Health(msg))
            }
            _ => Err(DeserializationError::UnknownRoom(room_id.clone())),
        }
    }

    fn supported_rooms() -> Vec<RoomId> {
        vec![
            RoomId::from("intentconfig"),
            RoomId::from("memdb"),
            RoomId::from("health"),
        ]
    }
}

// ============================================================================
// Tests
// ============================================================================

#[test]
fn test_different_enums_supported_rooms() {
    // Validate that the two enums declare different supported rooms
    let collector_rooms = CollectorMessages::supported_rooms();
    let database_rooms = DatabaseMessages::supported_rooms();

    assert_eq!(collector_rooms.len(), 4);
    assert_eq!(database_rooms.len(), 3);

    // Collector has metrics
    assert!(collector_rooms.contains(&RoomId::from("metrics")));
    // Database does not
    assert!(!database_rooms.contains(&RoomId::from("metrics")));
}

#[test]
fn test_wire_format_compatibility() {
    // This test validates that the inner messages serialize identically
    // regardless of which enum wrapper they're in

    let collector_msg = CollectorMessages::Health(HealthMessage::Ping);
    let database_msg = DatabaseMessages::Health(HealthMessage::Ping);

    // Serialize both
    let collector_bytes = collector_msg.serialize_inner().unwrap();
    let database_bytes = database_msg.serialize_inner().unwrap();

    // CRITICAL: Bytes must be identical!
    assert_eq!(collector_bytes, database_bytes);

    // Deserialize Collector's bytes as Database enum
    let reconstructed =
        DatabaseMessages::deserialize_for_room(&RoomId::from("health"), &collector_bytes).unwrap();

    // Should match!
    assert_eq!(reconstructed, database_msg);
}

#[test]
fn test_cross_binary_message_round_trip() {
    // Test that a message serialized by Collector can be deserialized by Database

    // Collector creates a message
    let original = IntentConfigMessage::ConfigUpdate {
        targets: vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()],
        rate: 100,
    };

    // Wrap in Collector enum
    let collector_msg = CollectorMessages::IntentConfig(original.clone());

    // Serialize (simulating network send)
    let room_id = collector_msg.room_id();
    let bytes = collector_msg.serialize_inner().unwrap();

    // Database receives and deserializes
    let database_msg = DatabaseMessages::deserialize_for_room(&room_id, &bytes).unwrap();

    // Extract inner message
    match database_msg {
        DatabaseMessages::IntentConfig(msg) => {
            assert_eq!(msg, original);
        }
        _ => panic!("Wrong variant!"),
    }
}

#[test]
fn test_metrics_room_only_in_collector() {
    // Test that Database enum can't handle metrics messages

    let collector_msg = CollectorMessages::Metrics(MetricsMessage::GetStats);
    let room_id = collector_msg.room_id();
    let bytes = collector_msg.serialize_inner().unwrap();

    // Database tries to deserialize metrics message
    let result = DatabaseMessages::deserialize_for_room(&room_id, &bytes);

    // Should fail: Database doesn't have Metrics variant
    assert!(result.is_err());
    match result {
        Err(DeserializationError::UnknownRoom(_)) => {
            // Expected!
        }
        _ => panic!("Expected UnknownRoom error"),
    }
}

#[test]
fn test_all_common_rooms_compatible() {
    // Test that all common rooms (intentconfig, memdb, health) work bidirectionally

    // Test IntentConfig
    let intent_msg = IntentConfigMessage::Query;
    let collector_intent = CollectorMessages::IntentConfig(intent_msg.clone());
    let bytes = collector_intent.serialize_inner().unwrap();
    let database_intent =
        DatabaseMessages::deserialize_for_room(&RoomId::from("intentconfig"), &bytes).unwrap();
    match database_intent {
        DatabaseMessages::IntentConfig(msg) => assert_eq!(msg, intent_msg),
        _ => panic!("Wrong variant"),
    }

    // Test MemDB
    let memdb_msg = MemDBMessage::Store {
        key: "test".to_string(),
        value: "data".to_string(),
    };
    let collector_memdb = CollectorMessages::MemDB(memdb_msg.clone());
    let bytes = collector_memdb.serialize_inner().unwrap();
    let database_memdb =
        DatabaseMessages::deserialize_for_room(&RoomId::from("memdb"), &bytes).unwrap();
    match database_memdb {
        DatabaseMessages::MemDB(msg) => assert_eq!(msg, memdb_msg),
        _ => panic!("Wrong variant"),
    }

    // Test Health
    let health_msg = HealthMessage::Pong { uptime_seconds: 42 };
    let collector_health = CollectorMessages::Health(health_msg.clone());
    let bytes = collector_health.serialize_inner().unwrap();
    let database_health =
        DatabaseMessages::deserialize_for_room(&RoomId::from("health"), &bytes).unwrap();
    match database_health {
        DatabaseMessages::Health(msg) => assert_eq!(msg, health_msg),
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_room_id_mapping() {
    // Test that room_id() returns correct IDs for both enums

    assert_eq!(
        CollectorMessages::IntentConfig(IntentConfigMessage::Query).room_id(),
        RoomId::from("intentconfig")
    );
    assert_eq!(
        DatabaseMessages::IntentConfig(IntentConfigMessage::Query).room_id(),
        RoomId::from("intentconfig")
    );

    assert_eq!(
        CollectorMessages::Metrics(MetricsMessage::GetStats).room_id(),
        RoomId::from("metrics")
    );

    // Database doesn't have Metrics, so we can't test it
}
