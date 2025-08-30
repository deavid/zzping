use anyhow::Result;
use chrono::Utc;
use log::{debug, error, info};
use std::fs;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;
use zzping_lib::protocol::RawDataRecord;

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
                    debug!("No records to write, skipping.");
                    continue;
                }

                info!("60s timer triggered, writing {} records to disk.", buffer.len());

                // Take the records from the buffer, leaving it empty
                let records_to_write = std::mem::take(&mut buffer);

                match write_records_to_disk(&records_to_write, "data").await {
                    Ok(path) => info!("Successfully wrote {} records to {}", records_to_write.len(), path),
                    Err(e) => error!("Failed to write records to disk: {e}"),
                }
            }
        }
    }
}

async fn write_records_to_disk(records: &[RawDataRecord], data_dir: &str) -> Result<String> {
    // Ensure the data directory exists
    fs::create_dir_all(data_dir)?;

    // Generate a timestamped filename
    let now = Utc::now();
    let filename = format!("{}/{}.zzp1", data_dir, now.format("%Y%m%d-%H%M%S"));

    // Compress the data
    let compressed_data = zzping_lib::chunked_v1::compress_chunked_v1(records)?;

    // Write to the file
    fs::write(&filename, compressed_data)?;

    Ok(filename)
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_write_and_read_records() {
        // 1. Setup a temporary directory
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path().to_str().unwrap();

        // 2. Create sample data
        let records = vec![
            RawDataRecord {
                sent_nanos: 1_000_000_000,
                rtt_nanos: 20_000_000,
            },
            RawDataRecord {
                sent_nanos: 1_010_000_000,
                rtt_nanos: 25_000_000,
            },
            RawDataRecord {
                sent_nanos: 1_020_000_000,
                rtt_nanos: u64::MAX, // lost packet
            },
        ];

        // 3. Write records to disk
        let file_path_str = write_records_to_disk(&records, data_dir).await.unwrap();
        let file_path = std::path::Path::new(&file_path_str);
        assert!(file_path.exists());

        // 4. Read and decompress the data
        let compressed_data = fs::read(file_path).unwrap();
        let decompressed_records = crate::storage::chunked_v1::decompress_chunked_v1(&compressed_data).unwrap();

        // 5. Assert correctness
        assert_eq!(records.len(), decompressed_records.len());

        // The chunked_v1 format is lossy due to quantization, so we can't do a direct comparison.
        // But we can check if the values are reasonably close.
        for i in 0..records.len() {
            // Check sent_nanos with a small tolerance
            let sent_diff = records[i].sent_nanos.abs_diff(decompressed_records[i].sent_nanos);
            assert!(sent_diff < 1_000_000, "sent_nanos diverged too much"); // < 1ms tolerance

            // Check rtt_nanos
            if records[i].rtt_nanos == u64::MAX {
                assert_eq!(decompressed_records[i].rtt_nanos, u64::MAX, "Packet loss not preserved");
            } else {
                 let rtt_diff = records[i].rtt_nanos.abs_diff(decompressed_records[i].rtt_nanos);
                 // Quantization error should be within a certain percentage or a fixed value.
                 // The quantizer is more precise for smaller values.
                 let tolerance = (records[i].rtt_nanos as f64 * 0.05).max(100_000.0) as u64; // 5% or 0.1ms
                 assert!(rtt_diff < tolerance, "rtt_nanos diverged too much: original={}, decompressed={}, diff={}, tolerance={}", records[i].rtt_nanos, decompressed_records[i].rtt_nanos, rtt_diff, tolerance);
            }
        }
    }
}
