//! Handles the ingestion of `RawDataRecord`s from collector clients.

use crate::ingestion_item::IngestionItem;
use anyhow::Result;
use log::{debug, error, info};
use std::net::SocketAddr;
use tokio::sync::mpsc;
use zzping_lib::protocol::{read_handshake, read_record};

/// Manages a single TCP connection from a `zzping-collector` instance.
///
/// This function enters a loop, continuously reading length-prefixed `RawDataRecord`
/// messages from the stream using the helper function from `zzping_lib::protocol`.
/// Each successfully deserialized record is then sent to the central storage task
/// via the MPSC channel.
///
/// The loop terminates gracefully if the client closes the connection. Any other
/// I/O or deserialization error will terminate the loop and log an error.
///
/// # Arguments
/// * `stream` - The TCP stream connected to the collector.
/// * `addr` - The socket address of the connected collector, for logging purposes.
/// * `tx` - The sender part of the MPSC channel to the storage task.
pub async fn handle_ingestion_connection<R>(
    mut stream: R,
    addr: SocketAddr,
    tx: mpsc::Sender<IngestionItem>,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin + Send,
{
    info!("Handling ingestion connection from {addr}");

    let handshake = match read_handshake(&mut stream).await? {
        Some(handshake) => handshake,
        None => {
            info!("Ingestion connection from {addr} closed before handshake.");
            return Ok(());
        }
    };
    info!(
        "Received handshake from {} for target {}",
        handshake.source_hostname, handshake.target
    );

    loop {
        let record = match read_record(&mut stream).await? {
            Some(record) => record,
            None => {
                info!("Ingestion connection from {addr} closed by peer.");
                break;
            }
        };

        debug!("Received record: {record:?}");

        let item = IngestionItem {
            source_hostname: handshake.source_hostname.clone(),
            target: handshake.target,
            record,
        };

        // Send the item to the storage task. If the channel is closed, it means
        // the storage task has panicked or shut down, so we can't continue.
        if let Err(e) = tx.send(item).await {
            error!("Failed to send record to storage task: {e}");
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokio::sync::mpsc;
    use zzping_lib::protocol::{ClientHandshake, RawDataRecord, write_handshake};

    #[tokio::test]
    async fn test_handle_ingestion_connection() {
        // 1. Create a mock handshake and record and serialize them into a buffer.
        let handshake = ClientHandshake {
            source_hostname: "test-host".to_string(),
            target: "1.1.1.1".parse().unwrap(),
        };
        let record = RawDataRecord {
            sent_nanos: 1,
            rtt_nanos: 2,
        };
        let mut buffer = Vec::new();
        write_handshake(&mut buffer, &handshake).await.unwrap();
        zzping_lib::protocol::write_record(&mut buffer, &record)
            .await
            .unwrap();
        let mut cursor = Cursor::new(buffer);

        // 2. Set up an MPSC channel to receive the item
        let (tx, mut rx) = mpsc::channel(1);

        // 3. Call the handler with the mock stream and channel
        let addr = "127.0.0.1:12345".parse().unwrap();
        let result = handle_ingestion_connection(&mut cursor, addr, tx).await;
        assert!(result.is_ok());

        // 4. Assert that the item was received on the channel
        let received = rx.recv().await.unwrap();
        assert_eq!(received.source_hostname, handshake.source_hostname);
        assert_eq!(received.target, handshake.target);
        assert_eq!(received.record, record);
    }
}
