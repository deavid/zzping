//! Message types for the Pinger component.
//!
//! Defines commands for target management, health monitoring, and configuration updates.

use actix::Message;
use serde::{Deserialize, Serialize};

use crate::error::PingerError;

/// Command to update ping targets
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), PingerError>")]
pub struct UpdateTargets {
    /// New list of targets to ping
    pub targets: Vec<TargetConfig>,
}

/// Configuration for a single ping target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetConfig {
    /// Target hostname or IP address
    pub target: String,
    /// Rate in milliseconds between pings
    pub rate_ms: u64,
    /// Timeout in milliseconds for ping responses
    pub timeout_ms: u64,
}

/// Command to pause or resume pinging
#[derive(Message, Debug)]
#[rtype(result = "()")]
pub struct SetPingingEnabled {
    /// Whether pinging should be enabled
    pub enabled: bool,
}

/// Get current pinger health status
#[derive(Message, Debug)]
#[rtype(result = "PingerHealth")]
pub struct GetHealth;

/// Health status of the pinger component
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
