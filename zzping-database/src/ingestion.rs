//! Handles the ingestion of `RawDataRecord`s from collector clients.

use anyhow::Result;
use log::{debug, error, info};
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use zzping_lib::protocol::{read_record, RawDataRecord};

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

        // Send the record to the storage task. If the channel is closed, it means
        // the storage task has panicked or shut down, so we can't continue.
        if let Err(e) = tx.send(record).await {
            error!("Failed to send record to storage task: {e}");
            break;
        }
    }
    Ok(())
}
