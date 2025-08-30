//! A mock `PingClient` for use in unit tests.

use crate::ping_client::{PingClient, PingResult};
use async_trait::async_trait;
use std::time::Instant;
use tokio::sync::{mpsc, OwnedSemaphorePermit};

/// A mock `PingClient` that does nothing.
///
/// This is used in unit tests to isolate the `PingerSession` logic from the
/// actual pinging implementation, which requires privileged access.
#[derive(Default)]
pub struct PingMockClient;

impl PingMockClient {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PingClient for PingMockClient {
    /// This mock implementation does nothing.
    ///
    /// The `permit` is passed in and immediately dropped, which simulates the
    /// semaphore permit being released after a ping operation completes. This
    /// is sufficient for testing the semaphore logic of `PingerSession`.
    async fn ping(
        &self,
        _sequence_idx: u16,
        _tx: mpsc::Sender<PingResult>,
        _permit: OwnedSemaphorePermit,
        _start_time: Instant,
    ) {
        // In a test environment, we do nothing. The permit is dropped when
        // this function returns, which is the desired behavior for testing
        // the semaphore logic.
    }
}
