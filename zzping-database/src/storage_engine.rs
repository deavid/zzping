//! The core storage engine for the database.
//!
//! This module contains the background task that receives ping data and handles
//! the process of buffering, compressing, and writing it to disk.

use crate::{finalization, ingestion_item::IngestionItem};
use anyhow::Result;
use chrono::{NaiveDate, Utc};
use log::{error, info};
use std::collections::HashMap;
use std::fs;
use std::net::IpAddr;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;
use zzping_lib::protocol::RawDataRecord;

struct StorageEngine {
    buffer: HashMap<(String, IpAddr), Vec<RawDataRecord>>,
    current_day: NaiveDate,
}

impl StorageEngine {
    fn new() -> Self {
        Self {
            buffer: HashMap::new(),
            current_day: Utc::now().date_naive(),
        }
    }

    fn handle_item(&mut self, item: IngestionItem) {
        self.buffer
            .entry((item.source_hostname, item.target))
            .or_default()
            .push(item.record);
    }

    async fn tick(&mut self, data_dir: &str) {
        if !self.buffer.is_empty() {
            info!(
                "60s timer triggered, writing records to disk for {} sources.",
                self.buffer.len()
            );
            let records_to_write = std::mem::take(&mut self.buffer);
            for ((source, target), records) in records_to_write {
                match write_records_to_disk(&source, target, &records, data_dir).await {
                    Ok(path) => info!("Successfully wrote {} records to {}", records.len(), path),
                    Err(e) => error!("Failed to write records for {source}-{target}: {e}"),
                }
            }
        }

        let now_day = Utc::now().date_naive();
        if now_day != self.current_day {
            info!(
                "Day changed from {} to {}. Finalizing previous day's file.",
                self.current_day, now_day
            );
            let previous_day_str = self.current_day.format("%Y%m%d").to_string();
            let file_path = Path::new(data_dir).join(format!("{previous_day_str}.zzp1"));
            if file_path.exists()
                && let Err(e) = finalization::finalize_file(&file_path)
            {
                error!("Failed to finalize file for day {previous_day_str}: {e}");
            }
            self.current_day = now_day;
        }
    }
}

pub async fn storage_task(mut rx: mpsc::Receiver<IngestionItem>) {
    info!("Storage task started.");
    let mut engine = StorageEngine::new();
    let mut ticker = interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            Some(item) = rx.recv() => {
                engine.handle_item(item);
            }
            _ = ticker.tick() => {
                engine.tick(crate::DATA_DIR).await;
            }
        }
    }
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
    use ntest::timeout;
    use tempfile::tempdir;

    #[tokio::test]
    #[timeout(1000)]
    async fn test_storage_engine_tick() -> Result<()> {
        let mut engine = StorageEngine::new();
        let item = IngestionItem {
            source_hostname: "test-host".to_string(),
            target: "1.1.1.1".parse()?,
            record: RawDataRecord {
                sent_nanos: 1,
                rtt_nanos: 2,
            },
        };
        engine.handle_item(item);

        let temp_dir = tempdir()?;
        let data_dir = temp_dir.path().to_str().unwrap();

        engine.tick(data_dir).await;

        let files: Vec<_> = fs::read_dir(data_dir)?.collect();
        assert_eq!(files.len(), 1);
        Ok(())
    }

    #[tokio::test]
    #[timeout(1000)]
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
