// src/components/zzmem-db/src/types.rs

//! Defines the shared data types for the memory database component.

use serde::{Deserialize, Serialize};

/// The status of a single ping attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PingStatus {
    /// The ping was successful and received a reply.
    /// The value is the round-trip-time in nanoseconds.
    Success(u64),
    /// The ping timed out without receiving a reply.
    Timeout,
    /// An IO error occurred during the ping.
    /// This can indicate things like "Network Unreachable" or "Permission Denied".
    IOError,
    /// An "orphaned" record, where the `sent` an event was recorded,
    /// but no corresponding `result` event was received before the batch was flushed.
    Partial,
    /// The ping interval was skipped for flow control reasons (e.g., backpressure).
    Skipped,
    /// A catch-all for any other status.
    Other,
}

/// The result of a single ping measurement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingResult {
    /// The target that was pinged (e.g., "8.8.8.8").
    pub target: String,
    /// The UNIX timestamp in nanoseconds when the ping was sent.
    pub sent_time_ns: u64,
    /// The outcome of the ping.
    pub status: PingStatus,
}
