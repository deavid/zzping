use anyhow::Result;
use log::{error, info};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use zzping_common::RawDataRecord;

mod ingestion;
mod query;
mod storage;
mod storage_engine;

const INGESTION_ADDR: &str = "127.0.0.1:7878";
const QUERY_ADDR: &str = "127.0.0.1:7879";

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

    let (tx, rx) = mpsc::channel::<RawDataRecord>(1024);

    // Spawn the storage task
    tokio::spawn(storage_engine::storage_task(rx));

    loop {
        tokio::select! {
            Ok((stream, addr)) = ingestion_listener.accept() => {
                info!("Accepted ingestion connection from {addr}");
                let tx_clone = tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = ingestion::handle_ingestion_connection(stream, tx_clone).await {
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
