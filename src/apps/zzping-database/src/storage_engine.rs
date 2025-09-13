//! The core storage engine for the database.
//!
//! This module contains the background task that receives ping data and handles
//! the process of buffering, compressing, and writing it to disk.

use crate::ingestion_item::IngestionItem;
use anyhow::Result;
use chrono::Utc;
use log::{error, info};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::net::IpAddr;
use tokio::sync::mpsc;
use zzping_lib::protocol::RawDataRecord;

const NANOS_PER_MINUTE: u64 = 60 * 1_000_000_000;

struct StorageEngine {
    /// The buffer groups records by source and target, and then by the minute
    /// they belong to. The BTreeMap ensures that minutes are sorted, making it
    /// easy to find completed chunks.
    buffer: HashMap<(String, IpAddr), BTreeMap<u64, Vec<RawDataRecord>>>,
    data_dir: String,
}

impl StorageEngine {
    fn new(data_dir: &str) -> Self {
        Self {
            buffer: HashMap::new(),
            data_dir: data_dir.to_string(),
        }
    }

    async fn handle_item(&mut self, item: IngestionItem) {
        let minute_timestamp = item.record.sent_nanos / NANOS_PER_MINUTE;
        let buffer_key = (item.source_hostname.clone(), item.target);

        // Add the new record to the buffer.
        self.buffer
            .entry(buffer_key.clone())
            .or_default()
            .entry(minute_timestamp)
            .or_default()
            .push(item.record);

        // Check for completed chunks and write them to disk.
        self.write_completed_chunks(&buffer_key, minute_timestamp)
            .await;
    }

    /// Writes any minute-based chunks that are now considered "complete".
    ///
    /// A chunk for a given minute is considered complete as soon as we receive
    /// a record for any subsequent minute. This method checks for this condition
    /// and writes all completed chunks to disk.
    async fn write_completed_chunks(&mut self, buffer_key: &(String, IpAddr), current_minute: u64) {
        let per_target_buffer = if let Some(b) = self.buffer.get_mut(buffer_key) {
            b
        } else {
            return;
        };

        // Find all minutes that are older than the current one.
        let completed_minutes: Vec<u64> = per_target_buffer
            .keys()
            .filter(|&&minute| minute < current_minute)
            .cloned()
            .collect();

        if completed_minutes.is_empty() {
            return;
        }

        info!(
            "New record for minute {} triggers finalization of {} previous minute(s) for key {:?}",
            current_minute,
            completed_minutes.len(),
            buffer_key
        );

        for minute in completed_minutes {
            if let Some(records) = per_target_buffer.remove(&minute) {
                let (source, target) = buffer_key;
                match write_records_to_disk(source, *target, &records, &self.data_dir).await {
                    Ok(path) => info!("Successfully wrote {} records to {}", records.len(), path),
                    Err(e) => error!("Failed to write records for {source}-{target}: {e}"),
                }
            }
        }
    }
}

pub async fn storage_task(mut item_rx: mpsc::Receiver<IngestionItem>, data_dir: String) {
    info!("Storage task started.");
    let mut engine = StorageEngine::new(&data_dir);

    while let Some(item) = item_rx.recv().await {
        info!("[DEBUG 3/4] storage_task received IngestionItem: {item:?}");
        engine.handle_item(item).await;
    }

    info!("Storage task finished.");
}

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

async fn write_records_to_disk(
    source: &str,
    target: IpAddr,
    records: &[RawDataRecord],
    data_dir: &str,
) -> Result<String> {
    info!(
        "[DEBUG 4/4] write_records_to_disk called with {} records for {} - {}",
        records.len(),
        source,
        target
    );
    if records.is_empty() {
        return Err(anyhow::anyhow!("No records to write."));
    }

    fs::create_dir_all(data_dir)?;

    let now = Utc::now();
    let filename = format!(
        "{}/{}-{}-{}.zzp1",
        data_dir,
        source,
        target,
        now.format("%Y%m%d")
    );
    let filepath = Path::new(&filename);

    if !filepath.exists() {
        info!("Creating new daily file: {filename}");
        let header = zzping_lib::chunked_v1::create_chunked_v1_header()?;
        fs::write(filepath, header)?;
    }

    let chunk_body = zzping_lib::chunked_v1::create_chunk_body(records)?;

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
    // Removed use ntest::timeout;
    use tempfile::tempdir;

    #[tokio::test]
    // Removed #[timeout(1000)]
    async fn test_write_records_to_disk_empty() -> Result<()> {
        let temp_dir = tempdir()?;
        let data_dir = temp_dir.path().to_str().unwrap();
        let source = "test-host";
        let target = "1.1.1.1".parse()?;
        let records = vec![];
        let result = write_records_to_disk(source, target, &records, data_dir).await;
        assert!(result.is_err());
        Ok(())
    }
}
