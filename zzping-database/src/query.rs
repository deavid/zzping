//! Handles query requests from `zzping-gui` clients.

use anyhow::Result;
use log::info;
use std::fs;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use zzping_lib::protocol::{write_records_batch, RawDataRecord};

/// Manages a single TCP connection from a `zzping-gui` instance.
///
/// For the MVP, this function handles a single, hardcoded request: `b"GET_LAST_MINUTE"`.
/// Upon receiving this request, it finds the most recently created `.zzp1` data file,
/// decompresses it, and sends the entire contents back to the client using the
/// centralized `write_records_batch` protocol helper.
///
/// # Protocol
/// - Client sends: `b"GET_LAST_MINUTE"`
/// - Server responds:
///   - A length-prefixed, bincode-serialized `Vec<RawDataRecord>`.
pub async fn handle_query_connection(mut stream: TcpStream) -> Result<()> {
    info!("Handling query connection.");

    // 1. Read the request
    let mut request_buf = [0; 16]; // "GET_LAST_MINUTE" is 15 bytes
    let n = stream.read(&mut request_buf).await?;
    let request = &request_buf[..n];

    if request != b"GET_LAST_MINUTE" {
        return Err(anyhow::anyhow!("Invalid query request: {:?}", request));
    }

    // 2. Find the most recent .zzp1 file
    let records = match find_latest_zzp1_file(crate::DATA_DIR)? {
        Some(path) => {
            info!("Found latest file: {path:?}");
            // 3. Read and decompress the data
            let compressed_data = fs::read(&path)?;
            let decompressed_records: Vec<RawDataRecord> =
                zzping_lib::chunked_v1::decompress_chunked_v1(&compressed_data)?;
            info!(
                "Decompressed {} records from {:?}",
                decompressed_records.len(),
                path
            );
            decompressed_records
        }
        None => {
            info!("No .zzp1 files found, sending empty response.");
            Vec::new()
        }
    };

    // 4. Serialize and send the response using the centralized helper
    write_records_batch(&mut stream, &records).await?;

    info!(
        "Successfully sent {} records to query client.",
        records.len()
    );

    Ok(())
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
    use std::thread::sleep;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
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
    fn test_find_latest_in_empty_dir() {
        let temp_dir = tempdir().unwrap();
        let p = temp_dir.path();
        let latest = find_latest_zzp1_file(p.to_str().unwrap()).unwrap();
        assert!(latest.is_none());
    }
}
