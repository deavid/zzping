//! The core storage engine for the database.
//!
//! This module contains the background task that receives ping data and handles
//! the process of buffering, compressing, and writing it to disk.

use crate::finalization;
use anyhow::Result;
use chrono::Utc;
use log::{error, info};
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
    let mut current_day = Utc::now().date_naive();

    loop {
        tokio::select! {
            Some(record) = rx.recv() => {
                buffer.push(record);
            }
            _ = ticker.tick() => {
                if !buffer.is_empty() {
                    info!("60s timer triggered, writing {} records to disk.", buffer.len());
                    let records_to_write = std::mem::take(&mut buffer);
                    match write_records_to_disk(&records_to_write, crate::DATA_DIR).await {
                        Ok(path) => info!("Successfully wrote {} records to {}", records_to_write.len(), path),
                        Err(e) => error!("Failed to write records to disk: {e}"),
                    }
                }

                // Check for day change to trigger finalization
                let now_day = Utc::now().date_naive();
                if now_day != current_day {
                    info!("Day changed from {} to {}. Finalizing previous day's file.", current_day, now_day);
                    let previous_day_str = current_day.format("%Y%m%d").to_string();
                    let file_path = Path::new(crate::DATA_DIR).join(format!("{}.zzp1", previous_day_str));
                    if file_path.exists() {
                        if let Err(e) = finalization::finalize_file(&file_path) {
                            error!("Failed to finalize file for day {}: {}", previous_day_str, e);
                        }
                    }
                    current_day = now_day;
                }
            }
        }
    }
}

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// Compresses a slice of records and appends them as a new chunk to the appropriate daily file.
///
/// This function implements an append-only strategy for writing data. It determines the
/// correct file based on the current date (one file per day). If the file doesn't exist,
/// it creates it and writes a blank `chunked_v1` header. It then appends the compressed
/// records as a new chunk.
///
/// TODO: This does not yet handle splitting data by src/dst pairs as the architecture suggests.
/// TODO: This does not yet implement the header finalization step (rewriting stats/indexes).
async fn write_records_to_disk(records: &[RawDataRecord], data_dir: &str) -> Result<String> {
    if records.is_empty() {
        return Err(anyhow::anyhow!("No records to write."));
    }

    // Ensure the data directory exists.
    fs::create_dir_all(data_dir)?;

    // Generate a filename based on the current date.
    // NOTE: This does not yet account for src/dst pairs.
    let now = Utc::now();
    let filename = format!("{}/{}.zzp1", data_dir, now.format("%Y%m%d"));
    let filepath = Path::new(&filename);

    // Create the file with an empty header if it doesn't exist.
    if !filepath.exists() {
        info!("Creating new daily file: {filename}");
        let header = zzping_lib::chunked_v1::create_chunked_v1_header()?;
        fs::write(filepath, header)?;
    }

    // Compress the records into a new chunk body.
    let chunk_body = zzping_lib::chunked_v1::create_chunk_body(records)?;

    // Open the file in append mode and write the chunk.
    let mut file = OpenOptions::new().append(true).open(filepath)?;
    file.write_all(&chunk_body)?;

    info!(
        "Appended {} bytes ({} records) to {}",
        chunk_body.len(),
        records.len(),
        filename
    );

    Ok(filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use tempfile::tempdir;
    use zzping_lib::chunked_v1::{
        calculate_file_header_crc32, decompress_chunked_v1, ChunkHeader, FileHeader, IndexEntry,
        FILE_MAGIC, FORMAT_VERSION, HEADER_SIZE,
    };

    #[tokio::test]
    async fn test_write_records_to_disk_append() -> Result<()> {
        // 1. Setup
        let temp_dir = tempdir()?;
        let data_dir = temp_dir.path().to_str().unwrap();

        // One minute in nanoseconds
        const ONE_MINUTE_NS: u64 = 60 * 1_000_000_000;

        let records_chunk_1 = vec![
            RawDataRecord {
                sent_nanos: ONE_MINUTE_NS + 100,
                rtt_nanos: 10,
            },
            RawDataRecord {
                sent_nanos: ONE_MINUTE_NS + 200,
                rtt_nanos: u64::MAX,
            },
        ];
        let records_chunk_2 = vec![
            RawDataRecord {
                sent_nanos: 2 * ONE_MINUTE_NS + 300,
                rtt_nanos: 20,
            },
            RawDataRecord {
                sent_nanos: 2 * ONE_MINUTE_NS + 400,
                rtt_nanos: 30,
            },
        ];

        // 2. Execution
        // Write the first chunk
        let filename = write_records_to_disk(&records_chunk_1, data_dir).await?;
        // Append the second chunk
        write_records_to_disk(&records_chunk_2, data_dir).await?;

        // 3. Verification
        let mut final_data = fs::read(&filename)?;
        assert!(final_data.len() > HEADER_SIZE);

        // Manually "finalize" the header for testing purposes.
        // In a real scenario, a separate process would do this after the day is over.
        let payload = &final_data[HEADER_SIZE..];
        let mut cursor = std::io::Cursor::new(payload);
        let mut aggregate_entries = Vec::new();
        let mut index_entries = Vec::new();

        // The chunk offset points to the start of the ChunkHeader, not the '*' delimiter.
        let chunk_1_offset = HEADER_SIZE as u64;
        cursor.set_position(1); // Skip delimiter to read header
        let chunk_header_1 = ChunkHeader::read(&mut cursor)?;
        aggregate_entries.push(chunk_header_1.rtt_stats);
        index_entries.push(IndexEntry {
            chunk_offset_bytes: chunk_1_offset + 1,
        });

        // Determine the start of the second chunk
        let chunk_1_header_len = cursor.position() - 1;
        let chunk_1_content_len = chunk_1_header_len
            + chunk_header_1.rtt_stream_len_bytes as u64
            + chunk_header_1.send_time_stream_len_bytes as u64;
        let chunk_1_total_len = 1 + chunk_1_content_len + 4 + 1; // start_delim + content + crc + end_delim
        let chunk_2_offset = chunk_1_offset + chunk_1_total_len;

        cursor.set_position(chunk_1_total_len + 1); // Skip chunk 1 and its start delimiter
        let chunk_header_2 = ChunkHeader::read(&mut cursor)?;
        aggregate_entries.push(chunk_header_2.rtt_stats);
        index_entries.push(IndexEntry {
            chunk_offset_bytes: chunk_2_offset + 1,
        });

        let mut file_header = FileHeader {
            magic: FILE_MAGIC,
            format_version: FORMAT_VERSION,
            start_time_unix_ns: records_chunk_1[0].sent_nanos,
            aggregate_entry_count: aggregate_entries.len() as u32,
            index_entry_count: index_entries.len() as u32,
            header_crc32: 0,
        };
        file_header.header_crc32 = calculate_file_header_crc32(&file_header);

        let mut header_cursor = std::io::Cursor::new(&mut final_data[..HEADER_SIZE]);
        file_header.write(&mut header_cursor)?;
        for entry in &aggregate_entries {
            entry.write(&mut header_cursor)?;
        }
        for entry in &index_entries {
            entry.write(&mut header_cursor)?;
        }

        // Decompress the finalized data
        let decompressed_records = decompress_chunked_v1(&final_data)?;

        let mut all_records = records_chunk_1.clone();
        all_records.extend_from_slice(&records_chunk_2);

        assert_eq!(decompressed_records, all_records);

        Ok(())
    }
}
