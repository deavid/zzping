//! Handles all network communication for the `zzping-gui` application.

use anyhow::Result;
use crossbeam_channel::Sender;
use log::{info, warn};
use std::time::Duration;
use tokio::net::TcpStream;
use zzping_lib::protocol::{QueryCommand, RawDataRecord, read_records_batch, write_command};

const QUERY_ADDR: &str = "127.0.0.1:7879";

/// The main loop for the network background task.
///
/// This function runs in a separate thread and is responsible for periodically
/// fetching data from the `zzping-database`. It runs in an infinite loop,
/// attempting to fetch data once per second. If a connection or fetch fails,
/// it logs a warning and retries after a 5-second delay.
///
/// # Arguments
/// * `tx` - The sender part of a `crossbeam-channel` used to send the fetched
///   data back to the main UI thread.
pub async fn fetch_data_loop(tx: Sender<Vec<RawDataRecord>>) {
    info!("Network task started.");
    loop {
        match try_fetch_data(&tx).await {
            Ok(_) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(e) => {
                warn!("Failed to fetch data: {e}. Retrying in 5 seconds.");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// Attempts to perform a single fetch-and-send operation.
///
/// This function connects to the database, sends the `GET_LAST_MINUTE` command,
/// and uses the centralized `read_records_batch` helper to read the response.
/// It then sends the resulting `Vec<RawDataRecord>` over the channel to the UI thread.
pub async fn try_fetch_data(tx: &Sender<Vec<RawDataRecord>>) -> Result<()> {
    let mut stream = TcpStream::connect(QUERY_ADDR).await?;
    info!("Connected to query port at {QUERY_ADDR}");

    write_command(&mut stream, &QueryCommand::GetLastHour).await?;
    info!("Sent GetLastHour command.");

    if let Some(records) = read_records_batch(&mut stream).await? {
        info!("Received {} records from database.", records.len());
        tx.send(records)
            .map_err(|e| anyhow::anyhow!("UI channel closed: {}", e))?;
    } else {
        info!("Stream closed by database, sending empty vec.");
        tx.send(Vec::new())
            .map_err(|e| anyhow::anyhow!("UI channel closed: {}", e))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use zzping_lib::protocol::{read_command, write_records_batch};

    #[tokio::test]
    async fn test_try_fetch_data() -> Result<()> {
        // 1. Setup: Spawn a mock server in the background.
        let expected_records = vec![
            RawDataRecord {
                sent_nanos: 1,
                rtt_nanos: 10,
            },
            RawDataRecord {
                sent_nanos: 2,
                rtt_nanos: u64::MAX,
            },
        ];
        let records_clone = expected_records.clone();

        // Use a oneshot channel to signal that the server is ready and pass the address.
        let (server_ready_tx, server_ready_rx) = oneshot::channel();

        let server_handle = tokio::spawn(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            // Signal that the server is bound and ready to accept connections.
            server_ready_tx.send(addr).unwrap();

            let (mut stream, _) = listener.accept().await.unwrap();

            // Mock server logic: read command, write back records.
            let command = read_command(&mut stream).await.unwrap();
            assert_eq!(command, QueryCommand::GetLastHour);
            write_records_batch(&mut stream, &records_clone)
                .await
                .unwrap();
        });

        // Wait for the server to be ready and get the address.
        let test_addr = server_ready_rx.await?;

        // 2. Execution: Inline the fetch logic with the test address.
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut stream = TcpStream::connect(test_addr).await?;
        info!("Connected to test query port at {test_addr}");

        write_command(&mut stream, &QueryCommand::GetLastHour).await?;
        info!("Sent GetLastHour command.");

        if let Some(records) = read_records_batch(&mut stream).await? {
            info!("Received {} records from test database.", records.len());
            tx.send(records)?;
        }

        // 3. Verification: Check that the correct data was received on the channel.
        let received_records = rx.recv()?;
        assert_eq!(received_records, expected_records);

        // Clean up the server task.
        server_handle.await?;

        Ok(())
    }
}
