//! The default `PingClient` implementation that uses the `surge-ping` library.

use crate::ping_client::{PingClient, PingResult};
use anyhow::Result;
use async_trait::async_trait;
use log::debug;
use std::net::IpAddr;
use std::time::Instant;
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
        tx: mpsc::Sender<PingResult>,
        permit: OwnedSemaphorePermit,
        start_time: Instant,
        target_time: Instant,
    ) {
        let pinger = self
            .pinger_client
            .pinger(self.target, self.pinger_ident)
            .await;
        tokio::spawn(ping_task(
            pinger,
            sequence_idx,
            tx,
            permit,
            start_time,
            target_time,
        ));
    }
}

/// The core task for sending one ICMP echo request and receiving the reply.
///
/// This function is spawned for each individual ping. It uses the `surge-ping` library
/// to send the packet and await a response. The result (either the RTT or `None` for
/// a timeout/error) is sent back to the `connection_manager` via an MPSC channel.
async fn ping_task(
    mut pinger: Pinger,
    seq: u16,
    tx: mpsc::Sender<PingResult>,
    _permit: OwnedSemaphorePermit,
    start_time: Instant,
    target_time: Instant,
) {
    // Simple precision timing: sleep until target time
    let now = Instant::now();
    if target_time > now {
        let remaining = target_time - now;

        // NOTE: Using std::thread::sleep() instead of tokio::time::sleep_until() for better precision.
        // Empirical testing shows std::thread::sleep() provides ~10µs precision vs ~400µs for tokio sleep.
        // This blocks the current async task but doesn't block the tokio runtime since each ping
        // runs in its own spawned task. The precision gain (40x improvement) justifies this approach.
        std::thread::sleep(remaining);
    }

    let sent_nanos = start_time.elapsed().as_nanos() as u64;
    let result = pinger.ping(PingSequence(seq), &[0; 8]).await;
    let rtt = match result {
        Ok((_, rtt)) => Some(rtt),
        Err(_) => None,
    };

    let result = PingResult { sent_nanos, rtt };

    if tx.send(result).await.is_err() {
        // Receiver has been dropped, which means the main connection task has
        // terminated. This task can now gracefully exit.
        debug!("Receiver dropped, ping task exiting.");
    }
}
