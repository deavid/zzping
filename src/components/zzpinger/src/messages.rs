//! Message types used by the zzpinger component.

use actix::Message;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::{Duration, Instant, SystemTime};

/// Message to update the intent configuration for pinging targets.
#[derive(Debug, Clone, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub struct UpdateIntentConfig {
    /// List of IP addresses to ping.
    pub targets: Vec<IpAddr>,
    /// Number of pings per second for each target.
    pub pings_per_second: u16,
}

/// Message to update the component state (enable/disable pinging).
#[derive(Debug, Clone, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub struct UpdateCState {
    /// Whether pinging is enabled.
    pub enable: bool,
}

/// Event representing a ping operation result.
#[derive(Debug, Clone, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub struct PingEvent {
    /// Target host that was pinged.
    pub target_host: IpAddr,
    /// Time when the ping was sent.
    pub sent_time: SystemTime,
    /// State of the ping result.
    pub state: PingState,
}

/// Possible states for a ping event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PingState {
    /// Ping has been sent but not yet received.
    InFlight,
    /// Ping timed out.
    TimedOut,
    /// Network error occurred.
    NetworkError,
    /// Ping received with round-trip time.
    ReceivedRTT(Duration),
}

/// Message to schedule pings for a batch of targets at a specific time.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct SchedulePings {
    /// Aligned system time for the ping.
    pub aligned_time: SystemTime,
    /// Instant for precise timing.
    pub instant: Instant,
    /// Duration to wait from `instant` before firing the pings.
    /// Backend should sleep until: `Instant::from_std(instant) + fire_duration` for precise timing.
    pub fire_duration: Duration,
    /// List of targets to ping.
    pub targets: Vec<IpAddr>,
}
