//! A mock `PingClient` for use in unit tests.

use crate::ping_client::{PingClient, PingResult};
use async_trait::async_trait;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, mpsc};

/// A mock `PingClient` that records calls to its `ping` method.
///
/// This is used in unit tests to isolate the `PingerSession` logic from the
/// actual pinging implementation and to verify that pings are being dispatched.
#[derive(Clone)]
pub struct PingMockClient {
    /// A shared, mutable vector that stores the sequence numbers of each ping call.
    /// Tests can inspect this vector to assert that `ping` was called.
    pub pings: Arc<Mutex<Vec<u16>>>,
    /// An optional `PingResult` to send back to the caller. If `None`, no result is sent.
    pub result_to_send: Option<PingResult>,
}

impl PingMockClient {
    pub fn new() -> Self {
        Self {
            pings: Arc::new(Mutex::new(Vec::new())),
            result_to_send: Some(PingResult {
                sent_nanos: 12345,
                rtt: Some(Duration::from_millis(50)),
            }),
        }
    }
}

impl Default for PingMockClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PingClient for PingMockClient {
    fn target(&self) -> IpAddr {
        // The mock client doesn't have a real target, so we return a dummy one.
        // This is sufficient for the tests that use this mock.
        IpAddr::from_str("127.0.0.1").unwrap()
    }

    /// This mock implementation records the sequence number of the ping call and
    /// optionally sends a pre-configured `PingResult` back to the `PingerSession`.
    ///
    /// The `permit` is passed in and immediately dropped, which simulates the
    /// semaphore permit being released after a ping operation completes.
    async fn ping(
        &self,
        sequence_idx: u16,
        tx: mpsc::Sender<PingResult>,
        _permit: OwnedSemaphorePermit,
        _start_time: Instant,
        _target_time: Instant,
    ) {
        self.pings.lock().unwrap().push(sequence_idx);
        if let Some(result) = self.result_to_send.clone() {
            tokio::spawn(async move {
                tx.send(result).await.ok();
            });
        }
    }
}
