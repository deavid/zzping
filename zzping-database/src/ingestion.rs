use anyhow::Result;
use log::{debug, error, info};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use zzping_common::RawDataRecord;

pub async fn handle_ingestion_connection(
    mut stream: TcpStream,
    tx: mpsc::Sender<RawDataRecord>,
) -> Result<()> {
    info!("Handling ingestion connection.");
    loop {
        // Read the length prefix (u32 Big Endian)
        let len = match stream.read_u32().await {
            Ok(len) => len,
            // If the stream is closed, exit the loop cleanly
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                info!("Ingestion connection closed by peer.");
                break;
            }
            Err(e) => return Err(e.into()),
        };

        // Allocate a buffer of the correct size
        let mut buffer = vec![0; len as usize];
        // Read the exact number of bytes for the JSON payload
        stream.read_exact(&mut buffer).await?;

        // Deserialize the JSON payload into a RawDataRecord
        let record: RawDataRecord = serde_json::from_slice(&buffer)?;

        debug!("Received record: {record:?}");

        // Send the record to the storage task
        if let Err(e) = tx.send(record).await {
            error!("Failed to send record to storage task: {e}");
            // If the channel is closed, we can't continue
            break;
        }
    }
    Ok(())
}
