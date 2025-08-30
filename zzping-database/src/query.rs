use anyhow::Result;
use log::info;
use std::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zzping_lib::protocol::RawDataRecord;

pub async fn handle_query_connection(mut stream: TcpStream) -> Result<()> {
    info!("Handling query connection.");

    // 1. Read the request
    let mut request_buf = [0; 16]; // "GET_LAST_MINUTE" is 15 bytes
    let n = stream.read(&mut request_buf).await?;
    let request = &request_buf[..n];

    if request != b"GET_LAST_MINUTE" {
        return Err(anyhow::anyhow!("Invalid query request"));
    }

    // 2. Find the most recent .zzp1 file
    let latest_file = find_latest_zzp1_file("data")?;
    let latest_file_path = match latest_file {
        Some(path) => path,
        None => {
            info!("No .zzp1 files found, sending empty response.");
            // Send a zero-length response
            stream.write_u32(0).await?;
            return Ok(());
        }
    };
    info!("Found latest file: {latest_file_path:?}");

    // 3. Read the file's contents
    let compressed_data = fs::read(&latest_file_path)?;

    // 4. Decompress the data
    let records: Vec<RawDataRecord> = zzping_lib::chunked_v1::decompress_chunked_v1(&compressed_data)?;
    info!("Decompressed {} records from {:?}", records.len(), latest_file_path);

    // 5. Serialize the Vec<RawDataRecord> to JSON
    let json_response = serde_json::to_vec(&records)?;

    // 6. Send the length-prefixed JSON data back
    stream.write_u32(json_response.len() as u32).await?;
    stream.write_all(&json_response).await?;
    stream.flush().await?;

    info!("Successfully sent {} records to query client.", records.len());

    Ok(())
}

fn find_latest_zzp1_file(dir: &str) -> Result<Option<std::path::PathBuf>> {
    let entries = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "zzp1"))
        .map(|e| e.path())
        .collect::<Vec<_>>();

    // Find the entry with the latest modification time
    let latest = entries.into_iter().max_by_key(|path| {
        path.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });

    Ok(latest)
}
