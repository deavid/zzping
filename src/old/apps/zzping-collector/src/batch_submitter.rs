//! Batch submission and background batching logic.

use crate::database_client::DatabaseClientTrait;
use crate::pinger::FinalizedPing;
use anyhow::Result;
use log::{debug, info, warn};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use zzping_proto::zzping::{CollectorRole, RawDataRecord, SendBatchRequest, send_batch_response};

/// Commands that can be sent to the BatchSubmitter.
#[derive(Debug)]
pub enum BatchSubmitterCommand {
    /// Update the collector role (Primary/Supervising) for this submitter.
    UpdateRole(CollectorRole),
    /// Request the current buffer size; reply via the provided oneshot.
    GetHealth(tokio::sync::oneshot::Sender<usize>),
    /// Initialize the ACK cursor to the provided timestamp (nanos).
    InitializeAckCursor(u64),
    /// Prune buffered records by fsync timestamp.
    PruneByFsync(u64),
    /// Instruct the submitter to perform a clean shutdown.
    Shutdown,
}

/// The BatchSubmitter is responsible for buffering ping results for a single
/// target and submitting them to the database in batches.
pub struct BatchSubmitter {
    collector_uuid: String,
    target_ip: IpAddr,
    grace_period: Duration,
    buffer_limit: usize,
    retention_period: Duration,
    db_client: Arc<dyn DatabaseClientTrait>,
    buffer: BTreeMap<u64, RawDataRecord>,
    last_acked_received_nanos: u64,
    command_rx: mpsc::Receiver<BatchSubmitterCommand>,
    is_active: bool,
    // Test-only: unique id to help distinguish instances in logs (time-based).
    instance_id: u64,
}

impl BatchSubmitter {
    /// Creates a new BatchSubmitter.
    pub fn new(
        collector_uuid: String,
        target_ip: IpAddr,
        grace_period: Duration,
        buffer_limit: usize,
        retention_period: Duration,
        db_client: Arc<dyn DatabaseClientTrait>,
        command_rx: mpsc::Receiver<BatchSubmitterCommand>,
    ) -> Self {
        let instance_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Self {
            collector_uuid,
            target_ip,
            grace_period,
            buffer_limit,
            retention_period,
            db_client,
            buffer: BTreeMap::new(),
            last_acked_received_nanos: 0,
            command_rx,
            is_active: true, // Start active
            instance_id,
        }
    }

    /// Runs the BatchSubmitter's main loop.
    pub async fn run(mut self, mut results_rx: mpsc::Receiver<FinalizedPing>) -> Result<()> {
        info!("BatchSubmitter task started for target {}.", self.target_ip);

        let mut submission_interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                Some(finalized_ping) = results_rx.recv() => {
                    debug!("BatchSubmitter for {} received finalized ping: {:?}", self.target_ip, finalized_ping);
                    self.ingest_ping_result(finalized_ping);
                }
                Some(command) = self.command_rx.recv() => {
                    match command {
                        BatchSubmitterCommand::UpdateRole(role) => {
                            // The submitter should send data when it's the PRIMARY, or when it's a SUPERVISING
                            // instance that needs to drain its buffer.
                            self.is_active = matches!(role, CollectorRole::Primary | CollectorRole::Supervising);
                            info!(
                                "BatchSubmitter for {} role updated. Active: {}.",
                                self.target_ip, self.is_active
                            );
                        }
                        BatchSubmitterCommand::GetHealth(tx) => {
                            let buffer_size = self.buffer.len();
                            debug!("BatchSubmitter[id={}] GetHealth -> buffer_size={}", self.instance_id, buffer_size);
                            let _ = tx.send(buffer_size);
                        }
                        BatchSubmitterCommand::InitializeAckCursor(nanos) => {
                            info!(
                                "BatchSubmitter for {} initializing ACK cursor to {}.",
                                self.target_ip, nanos
                            );
                            self.last_acked_received_nanos = nanos;
                        }
                        BatchSubmitterCommand::PruneByFsync(fsync_nanos) => {
                            debug!("BatchSubmitter[id={}] PruneByFsync({})", self.instance_id, fsync_nanos);
                            info!("BatchSubmitter for {} pruning by fsync {}.", self.target_ip, fsync_nanos);
                            self.prune_by_fsync(fsync_nanos);
                            debug!(
                                "BatchSubmitter[id={}] PruneByFsync complete: buffer_size={}",
                                self.instance_id,
                                self.buffer.len()
                            );
                        }
                        BatchSubmitterCommand::Shutdown => {
                            info!(
                                "SHUTDOWN_LOG: BatchSubmitter for {} received shutdown command.",
                                self.target_ip
                            );
                            // Prevent the periodic tick from attempting to send while we
                            // proceed to the canonical final flush after the loop. The
                            // actual final send will be performed once outside the loop
                            // to ensure only one final batch is sent.
                            self.is_active = false;
                            info!(
                                "SHUTDOWN_LOG: BatchSubmitter for {} marked inactive; will perform final flush after loop.",
                                self.target_ip
                            );
                            break; // Signal to stop the loop
                        }
                    }
                }
                _ = submission_interval.tick() => {
                    self.prune_by_time();
                    if self.is_active
                        && let Err(e) = self.send_batch().await {
                            warn!("Failed to send batch for target {}: {}", self.target_ip, e);
                        }
                }
                else => {
                    break;
                }
            }
        }

        info!(
            "SHUTDOWN_LOG: BatchSubmitter for {} shutting down. Buffer has {} records. Attempting to send one final batch.",
            self.target_ip,
            self.buffer.len()
        );
        if let Err(e) = self.send_batch().await {
            warn!(
                "Failed to send final batch for target {}: {}",
                self.target_ip, e
            );
        } else {
            info!(
                "SHUTDOWN_LOG: Final batch sent successfully for target {}.",
                self.target_ip
            );
        }

        info!(
            "SHUTDOWN_LOG: BatchSubmitter for {} finished.",
            self.target_ip
        );
        Ok(())
    }

    /// Ingests a single `FinalizedPing` and stores it in the buffer.
    pub fn ingest_ping_result(&mut self, finalized_ping: FinalizedPing) {
        // Test-only trace (debug level): ingest event and key computation.
        debug!(
            "BatchSubmitter[id={}] ingest called: sent_nanos={} rtt={:?}",
            self.instance_id, finalized_ping.sent_nanos, finalized_ping.rtt
        );
        let (key, record) = if let Some(rtt) = finalized_ping.rtt {
            (
                finalized_ping.sent_nanos + rtt.as_nanos() as u64,
                RawDataRecord {
                    sent_nanos: finalized_ping.sent_nanos,
                    rtt_nanos: rtt.as_nanos() as u64,
                },
            )
        } else {
            (
                finalized_ping.sent_nanos + self.grace_period.as_nanos() as u64,
                RawDataRecord {
                    sent_nanos: finalized_ping.sent_nanos,
                    rtt_nanos: u64::MAX,
                },
            )
        };

        self.buffer.insert(key, record);

        debug!(
            "BatchSubmitter[id={}] buffer after insert (target={}): {}",
            self.instance_id,
            self.target_ip,
            self.buffer.len()
        );
        debug!(
            "BatchSubmitter for {} buffer size after insert: {}",
            self.target_ip,
            self.buffer.len()
        );

        if self.buffer.len() > self.buffer_limit
            && let Some((key, _)) = self.buffer.first_key_value()
        {
            let key = *key;
            self.buffer.remove(&key);
            warn!(
                "Buffer limit reached for target {}. Dropped oldest record with key {}.",
                self.target_ip, key
            );
        }
    }

    /// Sends a batch of records to the database.
    pub async fn send_batch(&mut self) -> Result<()> {
        // Changed to &self
        let now_ns = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u64;

        let records_to_send: Vec<RawDataRecord> = {
            self.buffer
                .range((self.last_acked_received_nanos + 1)..=now_ns)
                .map(|(_, record)| *record)
                .collect()
        };

        if records_to_send.is_empty() {
            return Ok(());
        }

        debug!(
            "Sending batch of {} records for target {}",
            records_to_send.len(),
            self.target_ip
        );

        let request = SendBatchRequest {
            collector_uuid: self.collector_uuid.clone(),
            target_ip: self.target_ip.to_string(),
            records: records_to_send,
            collector_believes_last_acked_received_nanos: self.last_acked_received_nanos,
        };

        let db_client = self.db_client.clone(); // Cloned the Arc
        let response = db_client.send_batch(request).await?.into_inner(); // Used cloned db_client

        match send_batch_response::Status::try_from(response.status)? {
            send_batch_response::Status::Ok => {
                self.last_acked_received_nanos =
                    response.database_confirms_last_acked_received_nanos;
                debug!(
                    "Batch for {} OK. New acked_nanos: {}",
                    self.target_ip, self.last_acked_received_nanos
                );
            }
            send_batch_response::Status::Desync => {
                warn!(
                    "DESYNC for target {}. DB expects records after {}. Rewinding.",
                    self.target_ip, response.database_confirms_last_acked_received_nanos
                );
                self.last_acked_received_nanos =
                    response.database_confirms_last_acked_received_nanos;
            }
        }

        Ok(())
    }

    /// Removes records from the buffer that are older than the retention period.
    pub fn prune_by_time(&mut self) {
        let now = SystemTime::now();
        let cutoff_time = now - self.retention_period;
        let cutoff_ns = cutoff_time
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        self.buffer.retain(|&key, _| key > cutoff_ns);
    }

    /// Removes records from the buffer that have been acknowledged as flushed
    /// to disk by the database.
    pub fn prune_by_fsync(&mut self, fsync_nanos: u64) {
        self.buffer.retain(|&key, _| key > fsync_nanos);
    }

    /// Returns the current size of the buffer (for testing purposes).
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns a copy of the buffer contents (for testing purposes).
    pub fn buffer_contents(&self) -> BTreeMap<u64, RawDataRecord> {
        self.buffer.clone()
    }

    /// Returns the last acked received nanos (for testing purposes).
    pub fn last_acked_received_nanos(&self) -> u64 {
        self.last_acked_received_nanos
    }
}
