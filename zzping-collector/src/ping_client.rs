//! Defines the interface for a swappable pinging client.

use async_trait::async_trait;
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, mpsc};

/// The result of a single ping operation.
#[derive(Debug, Clone)]
pub struct PingResult {
    /// The time at which the ping was sent, relative to the start of the session.
    pub sent_nanos: u64,
    /// The round-trip time, or `None` if the packet was lost.
    pub rtt: Option<Duration>,
}

/// A trait for a client that can send ICMP pings.
///
/// This abstraction allows for swapping the underlying ping implementation,
/// which is especially useful for testing.
#[async_trait]
pub trait PingClient: Send + Sync {
    /// Sends a single ping.
    ///
    /// This method is responsible for taking all the necessary information,
    /// performing the ping operation (likely in a separate task), and ensuring
    /// the provided semaphore permit is released when the operation is complete.
    async fn ping(
        &self,
        sequence_idx: u16,
        tx: mpsc::Sender<PingResult>,
        permit: OwnedSemaphorePermit,
        start_time: Instant,
        target_time: Instant,
    );
}
