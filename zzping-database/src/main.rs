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

mod finalization;
mod ingestion;
mod query;
mod storage_engine;

const INGESTION_ADDR: &str = "127.0.0.1:7878";
const QUERY_ADDR: &str = "127.0.0.1:7879";
pub const DATA_DIR: &str = "data";

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
        if path.extension().is_some_and(|ext| ext == "zzp1")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && let Ok(file_date) = NaiveDate::parse_from_str(stem, "%Y%m%d")
            && file_date < today
        {
            // This file is from a previous day, attempt to finalize it.
            if let Err(e) = finalization::finalize_file(&path) {
                error!("Failed to finalize file {:?}: {}", path, e);
            }
        }
    }

    info!("Startup finalization check complete.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use chrono::{Duration, Local};
    use std::fs;
    use tempfile::tempdir;
    use zzping_lib::{
        chunked_v1::{create_chunk_body, create_chunked_v1_header, decompress_chunked_v1},
        protocol::RawDataRecord,
    };

    #[test]
    fn test_startup_finalization() -> Result<()> {
        // 1. Setup
        let temp_dir = tempdir()?;
        let data_dir = temp_dir.path().to_str().unwrap();

        // Create an unfinalized file for yesterday
        let yesterday = Local::now().date_naive() - Duration::days(1);
        let filename = format!("{}.zzp1", yesterday.format("%Y%m%d"));
        let file_path = temp_dir.path().join(filename);

        let mut file_content = create_chunked_v1_header()?;
        let records = vec![RawDataRecord {
            sent_nanos: 1,
            rtt_nanos: 10,
        }];
        let chunk_body = create_chunk_body(&records)?;
        file_content.extend_from_slice(&chunk_body);
        fs::write(&file_path, file_content)?;

        // Create a finalized file for two days ago to ensure it's not touched
        let two_days_ago = Local::now().date_naive() - Duration::days(2);
        let finalized_filename = format!("{}.zzp1", two_days_ago.format("%Y%m%d"));
        let finalized_file_path = temp_dir.path().join(finalized_filename);
        let mut finalized_content = create_chunked_v1_header()?;
        let final_records = vec![RawDataRecord {
            sent_nanos: 99,
            rtt_nanos: 99,
        }];
        let final_chunk_body = create_chunk_body(&final_records)?;
        finalized_content.extend_from_slice(&final_chunk_body);
        fs::write(&finalized_file_path, finalized_content)?;
        // Manually finalize it before the test
        finalization::finalize_file(&finalized_file_path)?;
        let original_finalized_content = fs::read(&finalized_file_path)?;

        // 2. Execution
        run_startup_finalization(data_dir)?;

        // 3. Verification
        // Check that the old file is now finalized and readable
        let finalized_data = fs::read(&file_path)?;
        let decompressed = decompress_chunked_v1(&finalized_data)?;
        assert_eq!(decompressed, records);

        // Check that the already-finalized file was not modified
        let current_finalized_content = fs::read(&finalized_file_path)?;
        assert_eq!(current_finalized_content, original_finalized_content);

        Ok(())
    }
}
