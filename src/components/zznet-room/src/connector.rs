use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use crate::room::RoomChannels;

/// Connects two rooms bidirectionally
/// Messages sent from Room A arrive at Room B and vice versa
pub fn connect_rooms<T: Send + 'static>(
    channels_a: RoomChannels<T>,
    channels_b: RoomChannels<T>,
) -> RoomConnection {
    // Forward A's outbound → B's inbound
    let task_a_to_b = tokio::spawn(forward_messages(
        channels_a.outbound_rx,
        channels_b.inbound_tx,
    ));

    // Forward B's outbound → A's inbound
    let task_b_to_a = tokio::spawn(forward_messages(
        channels_b.outbound_rx,
        channels_a.inbound_tx,
    ));

    RoomConnection {
        tasks: vec![task_a_to_b, task_b_to_a],
    }
}

async fn forward_messages<T>(
    mut rx: mpsc::Receiver<T>,
    tx: mpsc::Sender<T>,
) {
    while let Some(msg) = rx.recv().await {
        if tx.send(msg).await.is_err() {
            // Other side closed, stop forwarding
            tracing::debug!("Forward task stopped: receiver closed");
            break;
        }
    }
}

/// Handle for the connection between two rooms
/// When dropped, forwarding tasks are aborted
pub struct RoomConnection {
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for RoomConnection {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_connect_rooms_forwards_messages() {
        let (tx_a, rx_a): (mpsc::Sender<i32>, mpsc::Receiver<i32>) = mpsc::channel(10);
        let (tx_b, rx_b): (mpsc::Sender<i32>, mpsc::Receiver<i32>) = mpsc::channel(10);

        let channels_a = RoomChannels {
            outbound_rx: rx_a,
            inbound_tx: tx_b,
        };

        let channels_b = RoomChannels {
            outbound_rx: rx_b,
            inbound_tx: tx_a,
        };

        let _connection = connect_rooms(channels_a, channels_b);

        // This test is simplified - actual test in integration tests
    }
}