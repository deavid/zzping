//! The main entry point for the `zzping-database` service.
//!
//! This service is the central hub of the zzping monitoring system. It is responsible for:
//! 1. Listening for incoming ping data from `zzping-collector` instances on one port.
//! 2. Listening for query requests from `zzping-gui` instances on another port.
//! 3. Receiving, buffering, and compressing ping data into the `.zzp1` format.
//! 4. Periodically writing the compressed data to disk.
//! 5. Serving recent data to GUI clients.
//!
//! The main function initializes the listeners and spawns separate tasks for handling
//! each connection and for the storage engine.

use anyhow::Result;
use log::{error, info};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use zzping_lib::protocol::RawDataRecord;

mod ingestion;
mod query;
mod storage_engine;

const INGESTION_ADDR: &str = "127.0.0.1:7878";
const QUERY_ADDR: &str = "127.0.0.1:7879";

/// The main function for the database service.
///
/// Sets up logging, binds the TCP listeners, creates a channel for data ingestion,
/// and enters an infinite loop to accept and handle new connections.
#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();
    info!("starting zzping-database server");

    let ingestion_listener = TcpListener::bind(INGESTION_ADDR).await?;
    info!("Listening for ingestion on {INGESTION_ADDR}");

    let query_listener = TcpListener::bind(QUERY_ADDR).await?;
    info!("Listening for queries on {QUERY_ADDR}");

    // This channel is the central pipeline for all incoming data from collectors.
    let (tx, rx) = mpsc::channel::<RawDataRecord>(1024);

    // The storage task runs in the background, consuming from the channel.
    tokio::spawn(storage_engine::storage_task(rx));

    // The main loop concurrently accepts both ingestion and query connections.
    loop {
        tokio::select! {
            Ok((stream, addr)) = ingestion_listener.accept() => {
                info!("Accepted ingestion connection from {addr}");
                let tx_clone = tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = ingestion::handle_ingestion_connection(stream, addr, tx_clone).await {
                        error!("Error handling ingestion connection from {addr}: {e}");
                    }
                });
            }
            Ok((stream, addr)) = query_listener.accept() => {
                info!("Accepted query connection from {addr}");
                tokio::spawn(async move {
                    if let Err(e) = query::handle_query_connection(stream).await {
                        error!("Error handling query connection from {addr}: {e}");
                    }
                });
            }
            else => {
                error!("Error accepting connection");
                break;
            }
        }
    }

    Ok(())
}
