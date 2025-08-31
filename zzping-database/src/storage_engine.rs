//! The core storage engine for the database.
//!
//! This module contains the background task that receives ping data and handles
//! the process of buffering, compressing, and writing it to disk.

use anyhow::Result;
use chrono::Utc;
use log::{debug, error, info};
use std::fs;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;
use zzping_lib::protocol::RawDataRecord;

/// The main task for the storage engine.
///
/// This function runs in an infinite loop and performs two main actions:
/// 1. Receives `RawDataRecord`s from the MPSC channel and adds them to an in-memory buffer.
/// 2. On a 60-second timer, it takes all records from the buffer, compresses them,
///    and writes them to a new timestamped `.zzp1` file on disk.
///
/// This approach batches disk writes to improve performance and reduce disk I/O,
/// as writing individual records would be highly inefficient.
///
/// # Arguments
/// * `rx` - The receiving end of the MPSC channel from the ingestion tasks.
pub async fn storage_task(mut rx: mpsc::Receiver<RawDataRecord>) {
    info!("Storage task started.");
    let mut buffer = Vec::new();
    let mut ticker = interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            Some(record) = rx.recv() => {
                buffer.push(record);
            }
            _ = ticker.tick() => {
                if buffer.is_empty() {
                    debug!("No records to write, skipping disk write.");
                    continue;
                }

                info!("60s timer triggered, writing {} records to disk.", buffer.len());

                // Atomically swap the buffer with a new empty one.
                let records_to_write = std::mem::take(&mut buffer);

                match write_records_to_disk(&records_to_write, crate::DATA_DIR).await {
                    Ok(path) => info!("Successfully wrote {} records to {}", records_to_write.len(), path),
                    Err(e) => error!("Failed to write records to disk: {e}"),
                }
            }
        }
    }
}

/// Compresses and writes a slice of records to a new timestamped file.
async fn write_records_to_disk(records: &[RawDataRecord], data_dir: &str) -> Result<String> {
    // Ensure the data directory exists.
    fs::create_dir_all(data_dir)?;

    // Generate a filename based on the current time.
    let now = Utc::now();
    let filename = format!("{}/{}.zzp1", data_dir, now.format("%Y%m%d-%H%M%S"));

    // Compress the data using the `chunked_v1` format.
    let compressed_data = zzping_lib::chunked_v1::compress_chunked_v1(records)?;

    // Write the compressed data to the file.
    fs::write(&filename, &compressed_data)?;

    info!("Wrote {} bytes to {}", compressed_data.len(), filename);

    Ok(filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_write_records_to_disk() -> Result<()> {
        // 1. Setup
        let temp_dir = tempdir()?;
        let data_dir = temp_dir.path().to_str().unwrap();
        let records = vec![
            RawDataRecord {
                sent_nanos: 1,
                rtt_nanos: 10,
            },
            RawDataRecord {
                sent_nanos: 2,
                rtt_nanos: u64::MAX,
            },
        ];

        // 2. Execution: Call the function to write the file
        let filename = write_records_to_disk(&records, data_dir).await?;

        // 3. Verification: Read the file back and decompress it
        let compressed_data = fs::read(filename)?;
        let decompressed_records = zzping_lib::chunked_v1::decompress_chunked_v1(&compressed_data)?;

        assert_eq!(decompressed_records, records);

        Ok(())
    }
}
