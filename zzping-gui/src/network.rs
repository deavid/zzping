//! Handles all network communication for the `zzping-gui` application.

use anyhow::Result;
use crossbeam_channel::Sender;
use log::{info, warn};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zzping_lib::protocol::RawDataRecord;

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
                warn!("Failed to fetch data: {}. Retrying in 5 seconds.", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// Attempts to perform a single fetch-and-send operation.
///
/// This function connects to the database, sends the `GET_LAST_MINUTE` command,
/// reads the length-prefixed JSON response, deserializes it, and sends the
/// resulting `Vec<RawDataRecord>` over the channel to the UI thread.
async fn try_fetch_data(tx: &Sender<Vec<RawDataRecord>>) -> Result<()> {
    let mut stream = TcpStream::connect(QUERY_ADDR).await?;
    info!("Connected to query port at {}", QUERY_ADDR);

    stream.write_all(GET_LAST_MINUTE_CMD).await?;
    info!("Sent GET_LAST_MINUTE command.");

    let len = stream.read_u32().await?;
    if len == 0 {
        info!("Received empty response from database.");
        tx.send(Vec::new())
            .map_err(|e| anyhow::anyhow!("UI channel closed: {}", e))?;
        return Ok(());
    }

    let mut buffer = vec![0; len as usize];
    stream.read_exact(&mut buffer).await?;
    info!("Received {} bytes from database.", len);

    let records: Vec<RawDataRecord> = serde_json::from_slice(&buffer)?;

    tx.send(records)
        .map_err(|e| anyhow::anyhow!("UI channel closed: {}", e))?;

    Ok(())
}
