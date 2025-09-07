use crate::database_client::DatabaseClient;
use crate::ping_client::PingResult;
use anyhow::Result;
use log::{debug, info, warn};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use zzping_proto::zzping::{send_batch_response, RawDataRecord, SendBatchRequest};

/// The BatchSubmitter is responsible for buffering ping results for a single
/// target and submitting them to the database in batches.
pub struct BatchSubmitter {
    collector_uuid: String,
    target_ip: IpAddr,
    pub grace_period: Duration,
    buffer_limit: usize,
    retention_period: Duration,
    db_client: DatabaseClient,
    pub buffer: BTreeMap<u64, RawDataRecord>,
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
    ) -> Self {
        Self {
            collector_uuid,
            target_ip,
            grace_period,
            buffer_limit,
            retention_period,
            db_client,
            buffer: BTreeMap::new(),
            last_acked_received_nanos: 0,
        }
    }

    /// Runs the BatchSubmitter's main loop.
    pub async fn run(mut self, mut results_rx: mpsc::Receiver<PingResult>) -> Result<()> {
        info!(
            "BatchSubmitter task started for target {}.",
            self.target_ip
        );

        let mut submission_interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                Some(ping_result) = results_rx.recv() => {
                    self.ingest_ping_result(ping_result);
                }
                _ = submission_interval.tick() => {
                    self.prune_by_time();
                    if let Err(e) = self.send_batch().await {
                        warn!("Failed to send batch for target {}: {}", self.target_ip, e);
                    }
                }
                else => {
                    // Pinger disconnected
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

    /// Ingests a single `PingResult` and stores it in the buffer.
    pub fn ingest_ping_result(&mut self, ping_result: PingResult) {
        let (key, record) = if let Some(rtt) = ping_result.rtt {
            (
                ping_result.sent_nanos + rtt.as_nanos() as u64,
                RawDataRecord {
                    sent_nanos: ping_result.sent_nanos,
                    rtt_nanos: rtt.as_nanos() as u64,
                },
            )
        } else {
            (
                ping_result.sent_nanos + self.grace_period.as_nanos() as u64,
                RawDataRecord {
                    sent_nanos: ping_result.sent_nanos,
                    rtt_nanos: u64::MAX,
                },
            )
        };
        self.buffer.insert(key, record);

        // Enforce the hard buffer limit by removing the oldest entry if full.
        if self.buffer.len() > self.buffer_limit {
            if let Some((key, _)) = self.buffer.first_key_value() {
                let key = *key;
                self.buffer.remove(&key);
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

        // Embargo: Only send records with a received_nanos key in the past.
        // We also only send records that are newer than the last ACKed record.
        let records_to_send: Vec<RawDataRecord> = self
            .buffer
            .range((self.last_acked_received_nanos + 1)..=now_ns)
            .map(|(_, record)| record.clone())
            .collect();

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
            collector_believes_last_acked_nanos: self.last_acked_received_nanos,
        };

        let response = self.db_client.send_batch(request).await?.into_inner();

        match send_batch_response::Status::try_from(response.status)? {
            send_batch_response::Status::Ok => {
                self.last_acked_received_nanos = response.database_confirms_last_acked_nanos;
                debug!(
                    "Batch for {} OK. New acked_nanos: {}",
                    self.target_ip, self.last_acked_received_nanos
                );
            }
            send_batch_response::Status::Desync => {
                warn!(
                    "DESYNC for target {}. DB expects records after {}. Rewinding.",
                    self.target_ip, response.database_confirms_last_acked_nanos
                );
                self.last_acked_received_nanos = response.database_confirms_last_acked_nanos;
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

        let original_len = self.buffer.len();
        self.buffer.retain(|&key, _| key > cutoff_ns);
        let removed_count = original_len - self.buffer.len();

        if removed_count > 0 {
            debug!(
                "Pruned {} records older than {:?} for target {}",
                removed_count, self.retention_period, self.target_ip
            );
        }
    }
}
