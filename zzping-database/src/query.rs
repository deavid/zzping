//! Handles query requests from `zzping-gui` clients.

use anyhow::Result;
use log::info;
use std::fs;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zzping_lib::protocol::RawDataRecord;

/// Manages a single TCP connection from a `zzping-gui` instance.
///
/// For the MVP, this function handles a single, hardcoded request: `b"GET_LAST_MINUTE"`.
/// Upon receiving this request, it finds the most recently created `.zzp1` data file,
/// decompresses it, and sends the entire contents back to the client as a
/// length-prefixed JSON array.
///
/// # Protocol
/// - Client sends: `b"GET_LAST_MINUTE"`
/// - Server responds:
///   - 4 bytes: `u32` length of the JSON payload, big-endian.
///   - N bytes: A JSON array of `RawDataRecord` structs.
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
    let latest_file_path = match find_latest_zzp1_file("data")? {
        Some(path) => path,
        None => {
            info!("No .zzp1 files found, sending empty response.");
            stream.write_u32(0).await?;
            return Ok(());
        }
    };
    info!("Found latest file: {latest_file_path:?}");

    // 3. Read and decompress the data
    let compressed_data = fs::read(&latest_file_path)?;
    let records: Vec<RawDataRecord> =
        zzping_lib::chunked_v1::decompress_chunked_v1(&compressed_data)?;
    info!(
        "Decompressed {} records from {:?}",
        records.len(),
        latest_file_path
    );

    // 4. Serialize and send the response
    let json_response = serde_json::to_vec(&records)?;
    stream.write_u32(json_response.len() as u32).await?;
    stream.write_all(&json_response).await?;
    stream.flush().await?;

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
