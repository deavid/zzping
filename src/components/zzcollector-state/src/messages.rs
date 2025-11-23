//! Defines the internal messages used by the `CStateActor`.

use crate::state::CollectorStateData;
use actix::Message;
use thiserror::Error;

/// A comprehensive error type for the `zzcollector-state` component.
#[derive(Error, Debug)]
pub enum CStateError {
    /// Returned when the actor is not configured for the requested operation.
    #[error("Invalid role: The actor is not configured as a {0}.")]
    InvalidRole(String),

    /// Returned when the Room is required but not configured.
    #[error("Room is not configured.")]
    NotConnected,

    /// Returned when the network connection is required but not configured.
    #[error("Network connection is not configured.")]
    NetworkNotConfigured,

    /// Returned when a message fails to send to another actor.
    #[error("Actor message send error: {0}")]
    MailboxError(#[from] actix::MailboxError),

    /// Returned on network send failures.
    #[error("Failed to send message to peer: {0}")]
    SendError(String),
}

/// Command to update health metrics from other components.
#[derive(Message, Default)]
#[rtype(result = "()")]
pub struct UpdateHealthMetrics {
    /// The total number of pings sent.
    pub pings_sent: Option<u64>,
    /// The total number of pings received.
    pub pings_received: Option<u64>,
    /// The total number of batches sent.
    pub batches_sent: Option<u64>,
    /// The timestamp of the last configuration update.
    pub last_config_update_ms: Option<u64>,
}

/// Query for the current, complete state of the collector.
#[derive(Message)]
#[rtype(result = "Result<CollectorStateData, CStateError>")]
pub struct GetCollectorState;

/// Command to force an immediate heartbeat transmission.
/// Primarily used for testing.
#[derive(Message)]
#[rtype(result = "Result<(), CStateError>")]
pub struct ForceHeartbeat;

/// Query for the component's health summary.
#[derive(Message, Debug)]
#[rtype(result = "Result<CStateHealth, CStateError>")]
pub struct GetHealth;

/// Command to trigger the cleanup of stale collectors.
#[derive(Message)]
#[rtype(result = "()")]
pub struct CleanupStaleCollectors;

/// A snapshot of the component's health.
#[derive(Debug, Clone, PartialEq)]
pub struct CStateHealth {
    /// Total number of heartbeats sent.
    pub heartbeats_sent: u64,
    /// Total number of heartbeats acknowledged by the database.
    pub heartbeats_acked: u64,
    /// Total number of heartbeats that failed to send.
    pub heartbeats_failed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensures the `UpdateHealthMetrics` message can be created with default values.
    #[test]
    fn test_update_health_metrics_default() {
        let metrics = UpdateHealthMetrics::default();
        assert!(metrics.pings_sent.is_none());
        assert!(metrics.pings_received.is_none());
        assert!(metrics.batches_sent.is_none());
        assert!(metrics.last_config_update_ms.is_none());
    }
}
