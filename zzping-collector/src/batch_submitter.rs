use crate::database_client::DatabaseClient;
use crate::pinger::FinalizedPing;
use anyhow::Result;
use log::{debug, info, warn};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use zzping_proto::zzping::{send_batch_response, RawDataRecord, SendBatchRequest};

pub type SharedBuffer = Arc<Mutex<BTreeMap<u64, RawDataRecord>>>;

/// The BatchSubmitter is responsible for buffering ping results for a single
/// target and submitting them to the database in batches.
pub struct BatchSubmitter {
    collector_uuid: String,
    target_ip: IpAddr,
    pub grace_period: Duration,
    buffer_limit: usize,
    retention_period: Duration,
    db_client: DatabaseClient,
    pub buffer: SharedBuffer,
    pub last_acked_received_nanos: u64,
}

impl BatchSubmitter {
    /// Creates a new BatchSubmitter.
    pub fn new(
        collector_uuid: String,
        target_ip: IpAddr,
        grace_period: Duration,
        buffer_limit: usize,
        retention_period: Duration,
        db_client: DatabaseClient,
        buffer: SharedBuffer,
    ) -> Self {
        Self {
            collector_uuid,
            target_ip,
            grace_period,
            buffer_limit,
            retention_period,
            db_client,
            buffer,
            last_acked_received_nanos: 0,
        }
    }

    /// Runs the BatchSubmitter's main loop.
    pub async fn run(mut self, mut results_rx: mpsc::Receiver<FinalizedPing>) -> Result<()> {
        info!(
            "BatchSubmitter task started for target {}.",
            self.target_ip
        );

        let mut submission_interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                Some(finalized_ping) = results_rx.recv() => {
                    info!("BatchSubmitter for {} received finalized ping: {:?}", self.target_ip, finalized_ping);
                    self.ingest_ping_result(finalized_ping);
                }
                _ = submission_interval.tick() => {
                    self.prune_by_time();
                    if let Err(e) = self.send_batch().await {
                        warn!("Failed to send batch for target {}: {}", self.target_ip, e);
                    }
                }
                else => {
                    break;
                }
            }
        }

        info!(
            "Pinger disconnected, BatchSubmitter for {} shutting down.",
            self.target_ip
        );
        Ok(())
    }

    /// Ingests a single `FinalizedPing` and stores it in the buffer.
    pub fn ingest_ping_result(&mut self, finalized_ping: FinalizedPing) {
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

        let mut buffer = self.buffer.lock().unwrap();
        buffer.insert(key, record);

        if buffer.len() > self.buffer_limit {
            if let Some((key, _)) = buffer.first_key_value() {
                let key = *key;
                buffer.remove(&key);
                warn!(
                    "Buffer limit reached for target {}. Dropped oldest record with key {}.",
                    self.target_ip, key
                );
            }
        }
    }

    /// Sends a batch of records to the database.
    pub async fn send_batch(&mut self) -> Result<()> {
        let now_ns = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u64;

        let records_to_send: Vec<RawDataRecord> = {
            let buffer = self.buffer.lock().unwrap();
            buffer
                .range((self.last_acked_received_nanos + 1)..=now_ns)
                .map(|(_, record)| record.clone())
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

        let response = self.db_client.send_batch(request).await?.into_inner();

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

        let mut buffer = self.buffer.lock().unwrap();
        buffer.retain(|&key, _| key > cutoff_ns);
    }

    /// Removes records from the buffer that have been acknowledged as flushed
    /// to disk by the database.
    pub fn prune_by_fsync(&mut self, fsync_nanos: u64) {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.retain(|&key, _| key > fsync_nanos);
    }
}
