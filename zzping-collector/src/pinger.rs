//! Contains the logic for performing a single ICMP ping.

use log::debug;
use std::time::Duration;
use surge_ping::{PingSequence, Pinger};
use tokio::sync::{mpsc, OwnedSemaphorePermit};

/// The result of a single ping operation.
#[derive(Debug, Clone)]
pub struct PingResult {
    /// The round-trip time, or `None` if the packet was lost.
    pub rtt: Option<Duration>,
}

/// The core task for sending one ICMP echo request and receiving the reply.
///
/// This function is spawned for each individual ping. It uses the `surge-ping` library
/// to send the packet and await a response. The result (either the RTT or `None` for
/// a timeout/error) is sent back to the `connection_manager` via an MPSC channel.
///
/// # Arguments
/// * `pinger` - The `Pinger` instance from `surge-ping`.
/// * `seq` - The ICMP sequence number for this ping.
/// * `tx` - The MPSC sender to send the `PingResult` back to the main task.
/// * `_permit` - An owned semaphore permit. The primary purpose of this argument
///   is to ensure that the semaphore count is correctly managed. When this task
///   completes, the `_permit` is dropped, automatically releasing its slot in the
///   semaphore and allowing a new ping task to be spawned. This is a crucial
///   mechanism for controlling the number of concurrent pings.
// FIXME: The original PoC used `std::thread::sleep` for higher precision timing.
// This implementation uses a simple `tokio::time::interval`, which has lower
// precision (~1ms). This is acceptable for the MVP but should be revisited if
// higher precision send times are needed for advanced analysis.
pub async fn ping_task(
    mut pinger: Pinger,
    seq: u16,
    tx: mpsc::Sender<PingResult>,
    _permit: OwnedSemaphorePermit,
) {
    let result = pinger.ping(PingSequence(seq), &[0; 8]).await;
    let rtt = match result {
        Ok((_, rtt)) => Some(rtt),
        Err(_) => None,
    };
    if tx.send(PingResult { rtt }).await.is_err() {
        // Receiver has been dropped, which means the main connection task has
        // terminated. This task can now gracefully exit.
        debug!("Receiver dropped, ping task exiting.");
    }
}
