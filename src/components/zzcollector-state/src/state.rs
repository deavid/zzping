//! Defines the internal state structures for the `CStateActor`.

use std::collections::HashMap;
use tokio::time::Instant;

/// Contains the state specific to a `Collector` role instance.
#[derive(Debug, Clone)]
pub struct CollectorStateData {
    /// The unique ID of this collector.
    pub collector_id: String,
    /// A random nonce generated at startup to uniquely identify this connection session.
    pub connection_nonce: u64,
    /// The time at which the actor was started.
    pub start_time: Instant,
    /// The total number of pings sent, as reported by other components.
    pub pings_sent: u64,
    /// The total number of pings received, as reported by other components.
    pub pings_received: u64,
    /// The total number of metric batches sent, as reported by other components.
    pub batches_sent: u64,
    /// The timestamp of the last configuration update, as reported by other components.
    pub last_config_update_ms: u64,
    /// The timestamp of the last heartbeat sent by this actor.
    pub last_heartbeat_sent_ms: u64,
    /// The timestamp of the last heartbeat acknowledgment received from the database.
    pub last_heartbeat_ack_ms: u64,
    /// True if the collector physically holds the TCP port lock on the host machine.
    pub has_local_lock: bool,
    /// True if the database has authorized this collector to be the master.
    pub database_authorized: bool,
}

impl CollectorStateData {
    /// Creates a new state for a collector.
    pub fn new(collector_id: String) -> Self {
        Self {
            collector_id,
            connection_nonce: generate_connection_nonce(),
            start_time: Instant::now(),
            pings_sent: 0,
            pings_received: 0,
            batches_sent: 0,
            last_config_update_ms: 0,
            last_heartbeat_sent_ms: 0,
            last_heartbeat_ack_ms: 0,
            has_local_lock: false,
            database_authorized: true, // Default to true as per directive
        }
    }
}

/// Generates a random u64 to uniquely identify a collector's connection session.
fn generate_connection_nonce() -> u64 {
    rand::random()
}

/// Contains the state specific to a `Database` role instance.
#[derive(Debug)]
pub(crate) struct DatabaseStateData {
    /// A map of tracked collectors, keyed by their collector ID.
    pub collectors: HashMap<String, TrackedCollector>,
    /// The number of milliseconds without a heartbeat before a collector is considered stale.
    pub stale_timeout_ms: u64,
    /// The maximum number of collectors to track.
    pub max_collectors: Option<usize>,
}

impl Default for DatabaseStateData {
    fn default() -> Self {
        Self {
            collectors: HashMap::new(),
            stale_timeout_ms: 300_000, // 5 minutes default
            max_collectors: None,
        }
    }
}

/// Represents a collector being tracked by the database.
#[derive(Debug, Clone)]
pub(crate) struct TrackedCollector {
    /// The unique ID of the collector.
    pub id: String,
    /// The timestamp of the last heartbeat received from this collector.
    pub last_seen_ms: u64,
    /// The uptime of the collector in seconds.
    pub uptime_secs: u64,
    /// The total number of pings sent by the collector.
    pub pings_sent: u64,
    /// The total number of pings received by the collector.
    pub pings_received: u64,
    /// The total number of metric batches sent by the collector.
    pub batches_sent: u64,
    /// The connection nonce of the collector.
    pub connection_nonce: u64,
}

impl TrackedCollector {
    /// Creates a new TrackedCollector.
    pub(crate) fn new(id: String, connection_nonce: u64) -> Self {
        Self {
            id,
            last_seen_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            uptime_secs: 0,
            pings_sent: 0,
            pings_received: 0,
            batches_sent: 0,
            connection_nonce,
        }
    }

    /// Updates the collector's heartbeat information.
    pub(crate) fn update_heartbeat(
        &mut self,
        uptime_secs: u64,
        pings_sent: u64,
        pings_received: u64,
        batches_sent: u64,
        _last_config_update_ms: u64,
    ) {
        self.last_seen_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.uptime_secs = uptime_secs;
        self.pings_sent = pings_sent;
        self.pings_received = pings_received;
        self.batches_sent = batches_sent;
        // Note: last_config_update_ms could be stored if needed
    }
}
