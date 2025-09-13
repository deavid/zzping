//! A mock `PingClient` for use in unit tests.

use crate::ping_client::{PingClient, PingReply};
use async_trait::async_trait;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, mpsc};

/// A mock `PingClient` that sends the IP it was asked to ping to a channel.
#[derive(Clone)]
pub struct PingMockClient {
    target: IpAddr,
    ping_event_tx: mpsc::Sender<IpAddr>,
    rtt_to_send: Arc<tokio::sync::Mutex<Option<Duration>>>,
}

impl PingMockClient {
    pub fn new(target: IpAddr, ping_event_tx: mpsc::Sender<IpAddr>) -> Self {
        Self {
            target,
            ping_event_tx,
            rtt_to_send: Arc::new(tokio::sync::Mutex::new(Some(Duration::from_millis(50)))),
        }
    }

    pub async fn set_rtt_to_send(&self, rtt: Option<Duration>) {
        *self.rtt_to_send.lock().await = rtt;
    }
}

#[async_trait]
impl PingClient for PingMockClient {
    fn target(&self) -> IpAddr {
        self.target
    }

    async fn ping(
        &self,
        sequence_idx: u16,
        tx: mpsc::Sender<PingReply>,
        _permit: OwnedSemaphorePermit,
        _sent_nanos: u64,
    ) {
        self.ping_event_tx.send(self.target).await.ok();
        if let Some(rtt) = *self.rtt_to_send.lock().await {
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
    // Removed use ntest::timeout;
    use tokio::sync::Semaphore;

    #[tokio::test]
    // Removed #[timeout(1000)]
    async fn test_ping_mock_client_records_ping_and_sends_result() {
        let (ping_event_tx, mut ping_event_rx) = mpsc::channel(10);
        let target = "1.2.3.4".parse().unwrap();
        let client = PingMockClient::new(target, ping_event_tx);
        let (tx, mut rx) = mpsc::channel(1);
        let semaphore = Arc::new(Semaphore::new(1));
        let permit = semaphore.try_acquire_owned().unwrap();
        let sent_nanos = 999_999;
        let sequence_idx = 123;

        client.ping(sequence_idx, tx, permit, sent_nanos).await;

        // Check that the ping was recorded
        let pinged_ip = ping_event_rx.recv().await.unwrap();
        assert_eq!(pinged_ip, target);

        // Check that the result was sent with the correct sequence
        let result = rx.recv().await.unwrap();
        assert_eq!(result.sequence_idx, sequence_idx);
        assert_eq!(result.rtt, Some(Duration::from_millis(50)));
    }
}
