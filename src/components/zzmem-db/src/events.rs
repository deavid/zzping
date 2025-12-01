//! Event definitions for the MemDB component.
//!
//! Events originate from the MainActor and are consumed by NetworkActors via
//! the event bus. This enables a clean separation between business logic and
//! networking concerns while keeping the fan-out mechanism simple.

use crate::types::PingResult;
use actix::Message;

/// Events that MemDB publishes on its broadcast bus.
#[derive(Clone, Debug, Message)]
#[rtype(result = "()")]
pub enum MemDBEvent {
    /// Collector role buffered enough results and is ready to ship a batch.
    BatchReady {
        /// Timestamp associated with the batch (milliseconds since epoch).
        timestamp_ms: u64,
        /// The ping results contained in the batch.
        results: Vec<PingResult>,
    },
}
