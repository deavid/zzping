//! Handles all network communication for the `zzping-gui` application.

use anyhow::Result;
use crossbeam_channel::Sender;
use log::{info, warn};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use zzping_lib::protocol::{read_records_batch, RawDataRecord};

const QUERY_ADDR: &str = "127.0.0.1:7879";
const GET_LAST_MINUTE_CMD: &[u8] = b"GET_LAST_MINUTE";

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
async fn try_fetch_data(tx: &Sender<Vec<RawDataRecord>>) -> Result<()> {
    let mut stream = TcpStream::connect(QUERY_ADDR).await?;
    info!("Connected to query port at {QUERY_ADDR}");

    stream.write_all(GET_LAST_MINUTE_CMD).await?;
    info!("Sent GET_LAST_MINUTE command.");

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
