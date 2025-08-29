use log::debug;
use std::time::Duration;
use surge_ping::{PingSequence, Pinger};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct PingResult {
    pub rtt: Option<Duration>,
}

pub async fn ping_task(
    mut pinger: Pinger,
    seq: u16,
    tx: mpsc::Sender<PingResult>,
    _permit: tokio::sync::OwnedSemaphorePermit,
) {
    let result = pinger.ping(PingSequence(seq), &[0; 8]).await;
    let rtt = match result {
        Ok((_, rtt)) => Some(rtt),
        Err(_) => None,
    };
    if tx.send(PingResult { rtt }).await.is_err() {
        // Receiver has been dropped, task can exit.
        debug!("Receiver dropped, ping task exiting.");
    }
}
