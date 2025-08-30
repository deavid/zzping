//! A mock `PingClient` for use in unit tests.

use crate::ping_client::{PingClient, PingResult};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::{mpsc, OwnedSemaphorePermit};

/// A mock `PingClient` that records calls to its `ping` method.
///
/// This is used in unit tests to isolate the `PingerSession` logic from the
/// actual pinging implementation and to verify that pings are being dispatched.
#[derive(Default, Clone)]
pub struct PingMockClient {
    /// A shared, mutable vector that stores the sequence numbers of each ping call.
    /// Tests can inspect this vector to assert that `ping` was called.
    pub pings: Arc<Mutex<Vec<u16>>>,
}

impl PingMockClient {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PingClient for PingMockClient {
    /// This mock implementation records the sequence number of the ping call.
    ///
    /// The `permit` is passed in and immediately dropped, which simulates the
    /// semaphore permit being released after a ping operation completes.
    async fn ping(
        &self,
        sequence_idx: u16,
        _tx: mpsc::Sender<PingResult>,
        _permit: OwnedSemaphorePermit,
        _start_time: Instant,
    ) {
        self.pings.lock().unwrap().push(sequence_idx);
    }
}
