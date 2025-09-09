//! Defines the interface for a swappable pinging client.

use async_trait::async_trait;
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, mpsc};

/// The result of a single ping operation.
///
/// The reply from a single low-level ping operation.
///
/// This struct contains only the information that the low-level ping client
/// can know about. The Pinger is responsible for re-associating this reply
/// with its original `sent_nanos` timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct PingReply {
    /// The sequence number of the ping, used for correlation.
    pub sequence_idx: u16,
    /// The round-trip time, or `None` if the packet was lost.
    pub rtt: Option<Duration>,
}

/// A trait for a client that can send ICMP pings.
///
/// This abstraction allows for swapping the underlying ping implementation,
/// which is especially useful for testing.
use std::net::IpAddr;

#[async_trait]
pub trait PingClient: Send + Sync {
    /// The IP address of the target this client is pinging.
    fn target(&self) -> IpAddr;

    /// Sends a single ping.
    ///
    /// This method is responsible for taking all the necessary information,
    /// performing the ping operation (likely in a separate task), and ensuring
    // the provided semaphore permit is released when the operation is complete.
    async fn ping(
        &self,
        sequence_idx: u16,
        tx: mpsc::Sender<PingReply>,
        permit: OwnedSemaphorePermit,
        sent_nanos: u64,
    );
}

/// A mock implementation of PingClient for testing purposes.
pub struct MockPingClient {
    target: IpAddr,
}

impl MockPingClient {
    pub fn new(target: IpAddr) -> Self {
        Self { target }
    }
}

#[async_trait]
impl PingClient for MockPingClient {
    fn target(&self) -> IpAddr {
        self.target
    }

    async fn ping(
        &self,
        sequence_idx: u16,
        tx: mpsc::Sender<PingReply>,
        permit: OwnedSemaphorePermit,
        _sent_nanos: u64,
    ) {
        // For mock, just send a fake reply immediately.
        let reply = PingReply {
            sequence_idx,
            rtt: Some(Duration::from_millis(10)),
        };
        let _ = tx.send(reply).await;
        // Release the permit
        drop(permit);
    }
}
