use anyhow::Result;
use crossbeam_channel::Sender;
use log::{info, warn};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zzping_lib::protocol::RawDataRecord;

const QUERY_ADDR: &str = "127.0.0.1:7879";
const GET_LAST_MINUTE_CMD: &[u8] = b"GET_LAST_MINUTE";

pub async fn fetch_data_loop(tx: Sender<Vec<RawDataRecord>>) {
    info!("Network task started.");
    loop {
        match try_fetch_data(&tx).await {
            Ok(_) => {
                // Data fetched successfully, wait a second before the next fetch.
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(e) => {
                warn!("Failed to fetch data: {}. Retrying in 5 seconds.", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn try_fetch_data(tx: &Sender<Vec<RawDataRecord>>) -> Result<()> {
    // 1. Connect to the database
    let mut stream = TcpStream::connect(QUERY_ADDR).await?;
    info!("Connected to query port at {}", QUERY_ADDR);

    // 2. Send the request
    stream.write_all(GET_LAST_MINUTE_CMD).await?;
    info!("Sent GET_LAST_MINUTE command.");

    // 3. Receive the length-prefixed response
    let len = stream.read_u32().await?;
    if len == 0 {
        info!("Received empty response from database.");
        // Send an empty vector to clear the GUI if there's no data
        tx.send(Vec::new()).map_err(|e| anyhow::anyhow!("Failed to send empty data to UI thread: {}", e))?;
        return Ok(());
    }

    let mut buffer = vec![0; len as usize];
    stream.read_exact(&mut buffer).await?;
    info!("Received {} bytes from database.", len);

    // 4. Deserialize the JSON
    let records: Vec<RawDataRecord> = serde_json::from_slice(&buffer)?;

    // 5. Send the data to the UI thread
    tx.send(records).map_err(|e| anyhow::anyhow!("Failed to send data to UI thread: {}", e))?;

    Ok(())
}
