//! Message types for the Pinger component.
//!
//! Defines commands for target management, health monitoring, and configuration updates.
//! Validation prevents invalid states that could cause hangs or resource leaks.

use actix::Message;
use serde::{Deserialize, Serialize};

use crate::error::PingerError;

/// Updates the list of targets to ping. Replaces all existing targets and cancels tasks for removed ones.
/// Ensures only valid configurations are accepted to maintain system stability and prevent resource leaks.
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), PingerError>")]
pub struct UpdateTargets {
    /// New list of targets to ping
    pub targets: Vec<TargetConfig>,
}

/// Configuration for a single ping target. Defines timing and addressing for ping operations.
/// Validation prevents zero rates or timeouts that could cause infinite loops or hangs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetConfig {
    /// Target hostname or IP address
    pub target: String,
    /// Rate in milliseconds between pings
    pub rate_ms: u64,
    /// Timeout in milliseconds for ping responses
    pub timeout_ms: u64,
}

/// Enables or disables all ping operations. Allows pausing monitoring without reconfiguration.
/// Useful for maintenance windows or when network conditions require temporary suspension.
#[derive(Message, Debug)]
#[rtype(result = "()")]
pub struct SetPingingEnabled {
    /// Whether pinging should be enabled
    pub enabled: bool,
}

/// Retrieves current health status of the pinger. Provides operational metrics for monitoring.
/// Enables external systems to track ping performance and target counts without side effects.
#[derive(Message, Debug)]
#[rtype(result = "PingerHealth")]
pub struct GetHealth;

/// Health snapshot of the pinger's current state. Includes counters and operational flags.
/// Used for monitoring and alerting on ping operation health and performance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingerHealth {
    /// Number of active targets being pinged
    pub active_targets: usize,
    /// Total number of pings sent since startup
    pub total_pings_sent: u64,
    /// Total number of ping responses received
    pub total_responses: u64,
    /// Whether pinging is currently enabled
    pub enabled: bool,
}
