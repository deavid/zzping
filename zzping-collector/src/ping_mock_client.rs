//! A mock `PingClient` for use in unit tests.

use crate::ping_client::{PingClient, PingReply};
use async_trait::async_trait;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, mpsc};

/// A mock `PingClient` that records calls to its `ping` method.
#[derive(Clone)]
pub struct PingMockClient {
    pub pings: Arc<Mutex<Vec<u16>>>,
    pub rtt_to_send: Option<Duration>,
}

impl PingMockClient {
    pub fn new() -> Self {
        Self {
            pings: Arc::new(Mutex::new(Vec::new())),
            rtt_to_send: Some(Duration::from_millis(50)),
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
        IpAddr::from_str("127.0.0.1").unwrap()
    }

    async fn ping(
        &self,
        sequence_idx: u16,
        tx: mpsc::Sender<PingReply>,
        _permit: OwnedSemaphorePermit,
        _sent_nanos: u64,
    ) {
        self.pings.lock().unwrap().push(sequence_idx);
        if let Some(rtt) = self.rtt_to_send {
            let result = PingReply {
                sequence_idx,
                rtt: Some(rtt),
            };
            tokio::spawn(async move {
                tx.send(result).await.ok();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ntest::timeout;
    use tokio::sync::Semaphore;

    #[tokio::test]
    #[timeout(1000)]
    async fn test_ping_mock_client_records_ping_and_sends_result() {
        let client = PingMockClient::new();
        let (tx, mut rx) = mpsc::channel(1);
        let semaphore = Arc::new(Semaphore::new(1));
        let permit = semaphore.try_acquire_owned().unwrap();
        let sent_nanos = 999_999; // This is now unused by the mock, but required by the trait
        let sequence_idx = 123;

        client.ping(sequence_idx, tx, permit, sent_nanos).await;

        // Check that the ping was recorded
        {
            let pings = client.pings.lock().unwrap();
            assert_eq!(*pings, vec![123]);
        }

        // Check that the result was sent with the correct sequence
        let result = rx.recv().await.unwrap();
        assert_eq!(result.sequence_idx, sequence_idx);
        assert_eq!(result.rtt, Some(Duration::from_millis(50)));
    }
}
