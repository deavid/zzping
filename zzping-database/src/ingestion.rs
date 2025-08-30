use anyhow::Result;
use log::{debug, error, info};
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use zzping_lib::protocol::{read_record, RawDataRecord};

pub async fn handle_ingestion_connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    tx: mpsc::Sender<RawDataRecord>,
) -> Result<()> {
    info!("Handling ingestion connection from {addr}");
    loop {
        let record = match read_record(&mut stream).await? {
            Some(record) => record,
            None => {
                info!("Ingestion connection from {addr} closed by peer.");
                break;
            }
        };

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
