use chrono::{DateTime, Duration, Utc};

/// A single ping data point with proper type safety for RTT values.
#[derive(Debug, Clone, Copy)]
pub struct DataPoint {
    /// The timestamp when this ping was sent
    pub time: DateTime<Utc>,
    /// Round-trip time, or None if the packet was lost
    pub rtt: Option<Duration>,
}
