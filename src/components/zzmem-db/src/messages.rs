//! Internal messages for the MemDB actor.
//!
//! These messages are used for communication within the same process,
//! typically between components or for internal actor coordination.

use crate::network_messages::PingResult;
use actix::Message;

/// Request current health status of the MemDB component.
#[derive(Message)]
#[rtype(result = "Result<MemDBHealth, MemDBError>")]
pub struct GetHealth;

/// Request statistics for a specific target.
#[derive(Message)]
#[rtype(result = "Result<TargetStats, MemDBError>")]
pub struct GetStats {
    /// Target host to get statistics for
    pub target: String,
}

/// Clear the buffer (collector role only).
#[derive(Message)]
#[rtype(result = "Result<(), MemDBError>")]
pub struct ClearBuffer;

/// Store a ping result (internal use).
#[derive(Message)]
#[rtype(result = "Result<(), MemDBError>")]
pub struct StorePingResult {
    /// The ping result to store
    pub result: PingResult,
}

/// Health information for the MemDB component.
#[derive(Debug, Clone)]
pub struct MemDBHealth {
    /// Current role of the component
    pub role: String,
    /// Current buffer size
    pub buffer_size: usize,
    /// Total results processed
    pub total_results: u64,
    /// Number of successful batches
    pub successful_batches: u64,
    /// Number of failed batches
    pub failed_batches: u64,
    /// Timestamp of last batch
    pub last_batch_ms: Option<u64>,
}

/// A command message to create the room for component-to-component messaging.
/// This should be called after the actor is started so we can get the proper Recipient.
#[derive(Message)]
#[rtype(result = "()")]
pub struct CreateRoom;

/// A request to get the room channels for component-to-component messaging.
/// This allows external services to wire up room-based communication with the actor.
#[derive(Message)]
#[rtype(result = "GetRoomChannelsResponse")]
pub struct GetRoomChannels;

/// Response containing the room channels if available.
#[derive(Clone)]
pub struct GetRoomChannelsResponse {
    /// The room channels for component-to-component messaging, wrapped in Arc for sharing.
    pub channels: Option<std::sync::Arc<zznet_room::room::RoomChannels>>,
}

/// Errors that can occur in MemDB operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum MemDBError {
    /// Component is not in the correct role for this operation
    #[error("Component is not in the correct role for this operation")]
    WrongRole,

    /// Buffer overflow: {0} results dropped
    #[error("Buffer overflow: {0} results dropped")]
    BufferOverflow(usize),

    /// Storage limit exceeded for target: {0}
    #[error("Storage limit exceeded for target: {0}")]
    StorageLimitExceeded(String),

    /// Query failed: {0}
    #[error("Query failed: {0}")]
    QueryError(String),

    /// Network operation failed: {0}
    #[error("Network operation failed: {0}")]
    NetworkError(String),

    /// Internal error: {0}
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Statistics for a specific target.
#[derive(Debug, Clone)]
pub struct TargetStats {
    /// Target host
    pub target: String,
    /// Number of results for this target
    pub result_count: usize,
    /// Average RTT in microseconds
    pub avg_rtt_us: Option<f64>,
    /// Packet loss percentage
    pub packet_loss_percent: f64,
    /// Last seen timestamp
    pub last_seen_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_derives() {
        // Test that our messages implement the required traits
        // Just verify they can be instantiated
        let _get_health = GetHealth {};

        let _get_stats = GetStats {
            target: "test".to_string(),
        };

        let _clear_buffer = ClearBuffer {};

        let _store_result = StorePingResult {
            result: PingResult {
                target: "8.8.8.8".to_string(),
                timestamp_ms: 1234567890,
                rtt_us: Some(15000),
                sequence: 42,
            },
        };
    }

    #[test]
    fn test_error_display() {
        let err = MemDBError::WrongRole;
        assert!(err.to_string().contains("correct role"));

        let err = MemDBError::BufferOverflow(5);
        assert!(err.to_string().contains("5 results dropped"));

        let err = MemDBError::StorageLimitExceeded("8.8.8.8".to_string());
        assert!(err.to_string().contains("8.8.8.8"));
    }
}
