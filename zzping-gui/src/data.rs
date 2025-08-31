//! Defines the core data structures used for plotting in the GUI.

use chrono::{DateTime, Duration, Utc};

/// A single, processed data point ready for visualization.
///
/// This struct is the primary input for the plot widget. It uses strongly-typed
/// `chrono` values for time and RTT to ensure correctness.
#[derive(Debug, Clone, Copy)]
pub struct DataPoint {
    /// The absolute timestamp when this ping was sent.
    pub time: DateTime<Utc>,
    /// The round-trip time for this ping.
    ///
    /// This is `None` if the packet was lost.
    pub rtt: Option<Duration>,
}
