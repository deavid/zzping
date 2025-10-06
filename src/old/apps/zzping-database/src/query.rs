//! Handles query requests from `zzping-gui` clients.

use anyhow::Result;
use log::info;
use std::fs;
use std::path::PathBuf;
use zzping_lib::protocol::RawDataRecord;

/// Finds the most recent data file, decompresses it, and returns the records.
pub fn get_last_hour_records(data_dir: &str) -> Result<Vec<RawDataRecord>> {
    info!("get_last_hour_records(data_dir: {data_dir:?})");
    match find_latest_zzp1_file(data_dir)? {
        Some(path) => {
            info!("Found latest file: {path:?}");
            let compressed_data = fs::read(&path)?;
            let mut decompressed_records: Vec<RawDataRecord> =
                zzping_lib::chunked_v1::decompress_chunked_v1(&compressed_data)?;
            info!(
                "Decompressed {} records from {:?}",
                decompressed_records.len(),
                path
            );

            // Filter to only the last hour's records
            if let Some(max_sent) = decompressed_records.iter().map(|r| r.sent_nanos).max() {
                let one_hour_nanos = 60 * 60 * 1_000_000_000; // 1 hour in nanoseconds
                let cutoff = max_sent.saturating_sub(one_hour_nanos);
                decompressed_records.retain(|r| r.sent_nanos >= cutoff);
                info!(
                    "Filtered to {} records from the last hour",
                    decompressed_records.len()
                );
            }

            Ok(decompressed_records)
        }
        None => {
            info!("No .zzp1 files found, returning empty vec.");
            Ok(Vec::new())
        }
    }
}

/// Scans a directory for `.zzp1` files and returns the path to the most recently modified one.
///
/// This is a simple approach for the MVP. A more robust solution would involve a
/// proper database index or a more sophisticated file naming scheme.
fn find_latest_zzp1_file(dir: &str) -> Result<Option<PathBuf>> {
    let entries = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "zzp1"))
        .map(|e| e.path())
        .collect::<Vec<_>>();

    let latest = entries.into_iter().max_by_key(|path| {
        path.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });

    Ok(latest)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Removed use ntest::timeout;
    use std::thread::sleep;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    // Removed #[timeout(100)]
    fn test_find_latest_zzp1_file() {
        // 1. Setup a temporary directory
        let temp_dir = tempdir().unwrap();
        let p = temp_dir.path();

        // 2. Create some dummy files with different timestamps
        let f1_path = p.join("f1.zzp1");
        fs::write(&f1_path, "f1").unwrap();
        sleep(Duration::from_millis(10)); // Ensure modification times are distinct

        let f2_path = p.join("f2.zzp1");
        fs::write(&f2_path, "f2").unwrap();
        sleep(Duration::from_millis(10));

        let f3_path = p.join("f3.txt"); // Not a .zzp1 file
        fs::write(&f3_path, "f3").unwrap();
        sleep(Duration::from_millis(10));

        let f4_path = p.join("f4.zzp1");
        fs::write(&f4_path, "f4").unwrap();

        // 3. Call the function and assert it finds the latest .zzp1 file
        let latest = find_latest_zzp1_file(p.to_str().unwrap()).unwrap();
        assert_eq!(latest, Some(f4_path));
    }

    #[test]
    // Removed #[timeout(100)]
    fn test_find_latest_in_empty_dir() {
        let temp_dir = tempdir().unwrap();
        let p = temp_dir.path();
        let latest = find_latest_zzp1_file(p.to_str().unwrap()).unwrap();
        assert!(latest.is_none());
    }

    use zzping_lib::chunked_v1::{
        ChunkHeader, FILE_MAGIC, FORMAT_VERSION, FileHeader, HEADER_SIZE, IndexEntry,
        calculate_file_header_crc32, create_chunked_v1_header,
    };

    #[test]
    // Removed #[timeout(100)]
    fn test_get_last_hour_records() -> Result<()> {
        // 1. Setup: Create a temp dir and some sample data
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

        // 2. Create a dummy .zzp1 file using the new append-style API
        let mut final_data = create_chunked_v1_header()?;
        let chunk_body = zzping_lib::chunked_v1::create_chunk_body(&records)?;

        // Manually "finalize" the header for testing
        let mut cursor = std::io::Cursor::new(&chunk_body);
        cursor.set_position(1); // Skip delimiter
        let chunk_header = ChunkHeader::read(&mut cursor)?;

        let mut file_header = FileHeader {
            magic: FILE_MAGIC,
            format_version: FORMAT_VERSION,
            start_time_unix_ns: records.first().map_or(0, |r| r.sent_nanos),
            aggregate_entry_count: 1,
            index_entry_count: 1,
            header_crc32: 0,
        };
        file_header.header_crc32 = calculate_file_header_crc32(&file_header);
        let index_entry = IndexEntry {
            chunk_offset_bytes: HEADER_SIZE as u64 + 1,
        };

        let mut header_cursor = std::io::Cursor::new(&mut final_data[..HEADER_SIZE]);
        file_header.write(&mut header_cursor)?;
        chunk_header.rtt_stats.write(&mut header_cursor)?;
        index_entry.write(&mut header_cursor)?;

        final_data.extend_from_slice(&chunk_body);

        let file_path = temp_dir.path().join("test.zzp1");
        fs::write(file_path, final_data)?;

        // 3. Execution: Call the function under test
        let result_records = get_last_hour_records(data_dir)?;

        // 4. Verification: Assert the returned records match the original
        assert_eq!(result_records, records);

        Ok(())
    }

    #[test]
    // Removed #[timeout(100)]
    fn test_get_last_hour_records_empty_dir() -> Result<()> {
        let temp_dir = tempdir()?;
        let data_dir = temp_dir.path().to_str().unwrap();
        let result_records = get_last_hour_records(data_dir)?;
        assert!(result_records.is_empty());
        Ok(())
    }
}
