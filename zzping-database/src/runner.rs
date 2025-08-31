use crate::{
    DATA_DIR, INGESTION_ADDR, finalization, ingestion, ingestion_item::IngestionItem, query,
    storage_engine,
};
use anyhow::Result;
use log::{error, info};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

const QUERY_ADDR: &str = "127.0.0.1:7879";

/// The main function for the database service.
///
/// Sets up logging, binds the TCP listeners, creates a channel for data ingestion,
/// and enters an infinite loop to accept and handle new connections.
pub async fn run() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()
        .ok();
    info!("starting zzping-database server");

    let ingestion_listener = TcpListener::bind(INGESTION_ADDR).await?;
    info!("Listening for ingestion on {INGESTION_ADDR}");

    let query_listener = TcpListener::bind(QUERY_ADDR).await?;
    info!("Listening for queries on {QUERY_ADDR}");

    // This channel is the central pipeline for all incoming data from collectors.
    let (tx, rx) = mpsc::channel::<IngestionItem>(1024);

    // The storage task runs in the background, consuming from the channel.
    tokio::spawn(storage_engine::storage_task(rx));

    // Run finalization for any old files on startup.
    if let Err(e) = run_startup_finalization(DATA_DIR) {
        error!("Startup finalization failed: {e}");
    }

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

/// Scans the data directory for `.zzp1` files from previous days and finalizes them.
fn run_startup_finalization(data_dir: &str) -> Result<()> {
    use chrono::{Local, NaiveDate};
    use std::fs;

    info!("Running startup finalization check...");
    let today = Local::now().date_naive();
    fs::create_dir_all(data_dir)?;

    for entry in fs::read_dir(data_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "zzp1") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let parts: Vec<&str> = stem.split('-').collect();
                if let Some(date_str) = parts.last() {
                    if let Ok(file_date) = NaiveDate::parse_from_str(date_str, "%Y%m%d") {
                        if file_date < today {
                            // This file is from a previous day, attempt to finalize it.
                            if let Err(e) = finalization::finalize_file(&path) {
                                error!("Failed to finalize file {path:?}: {e}");
                            }
                        }
                    }
                }
            }
        }
    }

    info!("Startup finalization check complete.");
    Ok(())
}
