// src/components/zzstorage/src/actor.rs

//! The main actor for the storage engine.

use crate::codec::{compress_batch, PingBatch};
use crate::fs::{FileHeader, FILE_MAGIC, FORMAT_VERSION};
use actix::{Actor, Context, Handler, Message};
use anyhow::{anyhow, Result};
use byteorder::{BigEndian, WriteBytesExt};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;

/// Configuration for the `StorageActor`.
#[derive(Debug, Clone)]
pub enum StorageConfig {
    /// In-memory storage, for testing purposes.
    Ephemeral,
    /// File-based storage for production.
    FileSystem {
        /// The path to the directory where storage files will be written.
        path: PathBuf,
    },
}

/// The main actor responsible for storing compressed ping data.
pub struct StorageActor {
    config: StorageConfig,
    /// In-memory buffer for compressed blobs, used only in Ephemeral mode.
    ephemeral_blobs: Vec<Vec<u8>>,
    /// Handle to the storage file, used only in FileSystem mode.
    file_handle: Option<File>,
    /// The number of blobs stored in the file.
    blob_count: u64,
    /// The last timestamp seen for each target.
    last_timestamps: std::collections::HashMap<String, u64>,
}

impl StorageActor {
    /// Creates a new `StorageActor`.
    pub fn new(config: StorageConfig) -> Self {
        Self {
            config,
            ephemeral_blobs: Vec::new(),
            file_handle: None,
            blob_count: 0,
            last_timestamps: std::collections::HashMap::new(),
        }
    }
}

impl Actor for StorageActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        if let StorageConfig::FileSystem { path } = &self.config {
            let file_path = path.join("storage.zzs2");
            let is_new_file = !file_path.exists();

            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&file_path)
                .expect("Failed to open storage file");

            if is_new_file {
                let header = FileHeader {
                    magic: *FILE_MAGIC,
                    format_version: FORMAT_VERSION,
                    blob_count: 0,
                };
                header.write(&mut file).expect("Failed to write new file header");
                self.blob_count = 0;
            } else {
                self.blob_count = crate::fs::scan_and_recover(&mut file).expect("Failed to scan and recover storage file");
                // Update the header with the correct count.
                file.seek(SeekFrom::Start(6)).expect("Failed to seek to update blob count");
                file.write_u64::<BigEndian>(self.blob_count).expect("Failed to write updated blob count");
            }

            self.file_handle = Some(file);
        }
    }

    fn stopping(&mut self, _ctx: &mut Self::Context) -> actix::Running {
        if let Some(mut file) = self.file_handle.take() {
            if let StorageConfig::FileSystem { .. } = &self.config {
                file.seek(SeekFrom::Start(6))
                    .and_then(|_| file.write_u64::<BigEndian>(self.blob_count))
                    .and_then(|_| file.sync_all())
                    .unwrap_or_else(|e| {
                        // In a real application, this should log an error.
                        eprintln!("Failed to write final header: {}", e);
                    });
            }
        }
        actix::Running::Stop
    }
}

/// A message to store a batch of ping results.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct StoreBatch(pub PingBatch);

impl Handler<StoreBatch> for StorageActor {
    type Result = Result<()>;

    fn handle(&mut self, msg: StoreBatch, _ctx: &mut Self::Context) -> Self::Result {
        for result in &msg.0 {
            let entry = self.last_timestamps.entry(result.target.clone()).or_insert(0);
            *entry = (*entry).max(result.sent_time_ns);
        }

        let compressed_blob = compress_batch(&msg.0)?;
        if compressed_blob.is_empty() {
            return Ok(());
        }

        match &mut self.config {
            StorageConfig::Ephemeral => {
                self.ephemeral_blobs.push(compressed_blob);
            }
            StorageConfig::FileSystem { .. } => {
                let handle = self.file_handle.as_mut().ok_or_else(|| anyhow!("File handle not available"))?;

                // Append the blob to the end of the file.
                handle.seek(SeekFrom::End(0))?;
                handle.write_u32::<BigEndian>(compressed_blob.len() as u32)?;
                handle.write_all(&compressed_blob)?;
                handle.sync_all()?; // Ensure data is written to disk for durability.

                self.blob_count += 1;
            }
        }
        Ok(())
    }
}

/// A test-only message to retrieve all stored blobs from an ephemeral actor.
#[derive(Message)]
#[rtype(result = "Result<Vec<Vec<u8>>>")]
pub struct GetStoredBlobs;

/// A test-only message to retrieve the current blob count.
#[derive(Message)]
#[rtype(result = "Result<u64>")]
pub struct GetBlobCount;

/// A message to get the last persisted timestamp for a given target.
#[derive(Message)]
#[rtype(result = "Result<u64>")]
pub struct GetLastTimestamp {
    pub target: String,
}

impl Handler<GetStoredBlobs> for StorageActor {
    type Result = Result<Vec<Vec<u8>>>;

    fn handle(&mut self, _msg: GetStoredBlobs, _ctx: &mut Self::Context) -> Self::Result {
        match self.config {
            StorageConfig::Ephemeral => Ok(self.ephemeral_blobs.clone()),
            StorageConfig::FileSystem { .. } => {
                Err(anyhow::anyhow!("GetStoredBlobs is only supported in Ephemeral mode."))
            }
        }
    }
}

impl Handler<GetBlobCount> for StorageActor {
    type Result = Result<u64>;

    fn handle(&mut self, _msg: GetBlobCount, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.blob_count)
    }
}

impl Handler<GetLastTimestamp> for StorageActor {
    type Result = Result<u64>;

    fn handle(&mut self, msg: GetLastTimestamp, _ctx: &mut Self::Context) -> Self::Result {
        Ok(*self.last_timestamps.get(&msg.target).unwrap_or(&0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{PingResult, PingStatus};
    use actix::Actor;
    use tempfile::tempdir;

    #[actix_rt::test]
    async fn test_ephemeral_storage_actor() {
        let config = StorageConfig::Ephemeral;
        let actor = StorageActor::new(config);
        let addr = actor.start();

        let batch = vec![
            PingResult {
                target: "8.8.8.8".to_string(),
                sent_time_ns: 100,
                status: PingStatus::Success(50_000),
            },
            PingResult {
                target: "8.8.8.8".to_string(),
                sent_time_ns: 200,
                status: PingStatus::Timeout,
            },
        ];

        // Send a batch to be stored.
        let store_result = addr.send(StoreBatch(batch)).await;
        assert!(store_result.is_ok());
        assert!(store_result.unwrap().is_ok());

        // Retrieve the stored blobs.
        let blobs_result = addr.send(GetStoredBlobs).await;
        assert!(blobs_result.is_ok());
        let blobs = blobs_result.unwrap().unwrap();

        // Check that one blob was stored.
        assert_eq!(blobs.len(), 1);
        assert!(!blobs[0].is_empty());
    }

    #[actix_rt::test]
    async fn test_get_blobs_fails_in_fs_mode() {
        let dir = tempdir().unwrap();
        let config = StorageConfig::FileSystem {
            path: dir.path().to_path_buf(),
        };
        let actor = StorageActor::new(config);
        let addr = actor.start();

        let blobs_result = addr.send(GetStoredBlobs).await;
        assert!(blobs_result.is_ok());
        assert!(blobs_result.unwrap().is_err());
    }

    #[actix_rt::test]
    async fn test_filesystem_storage_actor_writes_file() {
        let dir = tempdir().unwrap();
        let config = StorageConfig::FileSystem {
            path: dir.path().to_path_buf(),
        };
        let actor = StorageActor::new(config);
        let addr = actor.start();

        let batch = vec![PingResult {
            target: "1.1.1.1".to_string(),
            sent_time_ns: 100,
            status: PingStatus::Success(50_000),
        }];

        addr.send(StoreBatch(batch)).await.unwrap().unwrap();

        // Drop the address to signal the actor to shut down. When the actor's mailbox
        // is empty and all Addr instances are dropped, the actor will stop.
        drop(addr);
        // A small delay to allow the actor to shut down and drop its file handle.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let file_path = dir.path().join("storage.zzs2");
        assert!(file_path.exists());

        let mut file = File::open(file_path).unwrap();
        let header = FileHeader::read(&mut file).unwrap();

        assert_eq!(header.magic, *FILE_MAGIC);
        assert_eq!(header.format_version, FORMAT_VERSION);
        assert_eq!(header.blob_count, 1); // Header is now updated on graceful shutdown.
    }

    #[actix_rt::test]
    async fn test_finalizer_recovers_blobs() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("storage.zzs2");

        // --- Manually create a corrupted file ---
        let mut file = OpenOptions::new().write(true).create(true).open(&file_path).unwrap();

        // Write a header with an incorrect blob count (0, when there will be 2).
        let bad_header = FileHeader {
            magic: *FILE_MAGIC,
            format_version: FORMAT_VERSION,
            blob_count: 0,
        };
        bad_header.write(&mut file).unwrap();

        // Write two valid blobs.
        let blob1 = vec![1, 2, 3];
        file.write_u32::<BigEndian>(blob1.len() as u32).unwrap();
        file.write_all(&blob1).unwrap();

        let blob2 = vec![4, 5];
        file.write_u32::<BigEndian>(blob2.len() as u32).unwrap();
        file.write_all(&blob2).unwrap();

        drop(file); // Close the file.

        // --- Start the actor and let it recover ---
        let config = StorageConfig::FileSystem {
            path: dir.path().to_path_buf(),
        };
        let actor = StorageActor::new(config);
        let addr = actor.start();

        // --- Verify the recovered state ---
        let count_result = addr.send(GetBlobCount).await.unwrap().unwrap();
        assert_eq!(count_result, 2); // Should have found the 2 blobs.
    }

    #[actix_rt::test]
    async fn test_graceful_shutdown_updates_header() {
        let dir = tempdir().unwrap();
        let config = StorageConfig::FileSystem {
            path: dir.path().to_path_buf(),
        };
        let actor = StorageActor::new(config);
        let addr = actor.start();

        let batch = vec![PingResult {
            target: "1.1.1.1".to_string(),
            sent_time_ns: 100,
            status: PingStatus::Success(50_000),
        }];

        addr.send(StoreBatch(batch)).await.unwrap().unwrap();

        // Stop the actor gracefully by dropping the address.
        drop(addr);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let file_path = dir.path().join("storage.zzs2");
        let mut file = File::open(file_path).unwrap();
        let header = FileHeader::read(&mut file).unwrap();

        assert_eq!(header.blob_count, 1); // Header should be updated on clean shutdown.
    }
}
