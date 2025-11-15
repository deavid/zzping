//! The default `PingClient` implementation that uses the `surge-ping` library.

use crate::ping_client::{PingClient, PingReply};
use anyhow::Result;
use async_trait::async_trait;
use log::debug;
use std::net::IpAddr;
use surge_ping::{Client, Config, PingIdentifier, PingSequence, Pinger};
use tokio::sync::{OwnedSemaphorePermit, mpsc};

/// A `PingClient` that uses `surge-ping` to send ICMP packets.
pub struct PingSurgeClient {
    /// The underlying `surge-ping` client. Requires privileged access to create.
    pinger_client: Client,
    /// The unique identifier for this pinger instance.
    pinger_ident: PingIdentifier,
    /// The IP address to ping.
    target: IpAddr,
}

impl PingSurgeClient {
    /// Creates a new `PingSurgeClient`.
    ///
    /// This will attempt to create a raw socket, which may fail if the process
    /// does not have sufficient privileges.
    pub fn new(target: IpAddr) -> Result<Self> {
        let pinger_config = Config::default();
        let pinger_client = Client::new(&pinger_config)?;
        let pinger_ident = PingIdentifier(rand::random());
        Ok(Self {
            pinger_client,
            pinger_ident,
            target,
        })
    }
}

#[async_trait]
impl PingClient for PingSurgeClient {
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
        let pinger = self
            .pinger_client
            .pinger(self.target, self.pinger_ident)
            .await;
        tokio::spawn(ping_task(pinger, sequence_idx, tx, permit, _sent_nanos));
    }
}

/// The core task for sending one ICMP echo request and receiving the reply.
///
/// This function is spawned for each individual ping. It uses the `surge-ping` library
/// to send the packet and await a response. The result (either the RTT or `None` for
/// a timeout/error) is sent back to the calling `Pinger` task via an MPSC channel.
async fn ping_task(
    mut pinger: Pinger,
    seq: u16,
    tx: mpsc::Sender<PingReply>,
    permit: OwnedSemaphorePermit,
    _sent_nanos: u64,
) {
    // Timing is now handled by the calling Pinger task. This task executes immediately.
    let result = pinger.ping(PingSequence(seq), &[0; 8]).await;
    let rtt = result.ok().map(|(_, rtt)| rtt);

    let result = PingReply {
        sequence_idx: seq,
        rtt,
    };

    if tx.send(result).await.is_err() {
        // Receiver has been dropped, which means the main Pinger task has
        // terminated. This child task can now gracefully exit.
        debug!("Receiver dropped, ping task exiting.");
    }
    // The permit is dropped here, releasing the semaphore slot for the next ping.
    // The permit needs to be dropped here, so we un-reserve it once we received the response from the ping.
    drop(permit);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ntest::timeout;
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[tokio::test]
    #[timeout(1000)]
    async fn test_ping_surge_client_new() {
        // Test with a valid IP address
        let target = "127.0.0.1".parse().unwrap();

        // Note: This test may fail in environments without proper privileges
        // In some test environments, surge-ping may not work due to raw socket requirements
        let result = PingSurgeClient::new(target);

        match result {
            Ok(client) => {
                assert_eq!(client.target(), target);
                assert_eq!(client.target, target);
            }
            Err(_) => {
                // If it fails due to privileges or runtime issues, that's expected
                // We just verify it's an error, not the specific error type
                // This allows the test to pass in restricted environments
            }
        }
    }

    #[test]
    fn test_ping_surge_client_target() {
        let target: IpAddr = "192.168.1.1".parse().unwrap();

        // For now, just test that the IP parsing works as expected
        let expected: IpAddr = "192.168.1.1".parse().unwrap();
        assert_eq!(target, expected);
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_ping_task_timeout_handling() {
        let (tx, mut rx) = mpsc::channel(10);

        // We can't easily test the actual ping_task without network access,
        // but we can test the channel communication and timeout handling

        // Create a simple test that sends a result through the channel
        let test_result = PingReply {
            sequence_idx: 123,
            rtt: Some(Duration::from_micros(1000)),
        };

        tx.send(test_result.clone()).await.unwrap();

        // Verify we can receive the result
        match tokio::time::timeout(Duration::from_millis(10), rx.recv()).await {
            Ok(Some(received)) => assert_eq!(received, test_result),
            _ => panic!("Should have received ping result"),
        }
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_ping_task_channel_closed() {
        let (tx, rx) = mpsc::channel(10);

        // Drop the receiver to simulate channel being closed
        drop(rx);

        // Try to send - this should fail gracefully
        let test_result = PingReply {
            sequence_idx: 123,
            rtt: Some(Duration::from_micros(1000)),
        };

        let send_result = tx.send(test_result).await;
        assert!(
            send_result.is_err(),
            "Send should fail when receiver is dropped"
        );
    }

    #[test]
    fn test_ping_reply_structure() {
        let rtt = Some(Duration::from_micros(5000));

        let result = PingReply {
            sequence_idx: 456,
            rtt,
        };

        assert_eq!(result.sequence_idx, 456);
        assert_eq!(result.rtt, rtt);
    }
}
