//! Buffers and periodically submits ping results to the database.
//!
//! This module organizes ping results into per-target buffers and ensures
//! data consistency through acknowledgment (ACK) cursors and retries on
//! desynchronization (DESYNC). It balances memory usage and latency.

use crate::{database_client::SharedDatabaseClient, ping_client::PingResult};
use anyhow::Result;
use log::{error, info, warn};
use std::{collections::HashMap, net::IpAddr, time::Duration};
use tokio::sync::mpsc;
use zzping_proto::zzping::{send_batch_response, RawDataRecord, SendBatchRequest};

/// Buffers ping results and sends them to the database.
///
/// Organizes results by target IP, periodically flushes them, and manages
/// acknowledgment cursors to ensure data consistency.
pub struct BatchSubmitter {
    /// Shared database client for sending batches.
    client: SharedDatabaseClient,

    /// Receives ping results from worker tasks.
    ping_results_rx: mpsc::Receiver<PingResult>,

    /// Unique identifier for this collector instance.
    collector_uuid: String,

    /// Authentication token for database access.
    token: String,

    /// Per-target buffers for storing ping records.
    buffers: HashMap<IpAddr, std::collections::VecDeque<RawDataRecord>>,

    /// Tracks the last acknowledged timestamp for each target.
    last_acked_nanos: HashMap<IpAddr, u64>,
}

impl BatchSubmitter {
    /// Creates a new batch submitter with the given configuration.
    ///
    /// Initializes buffers and acknowledgment tracking for each target.
    pub fn new(
        client: SharedDatabaseClient,
        ping_results_rx: mpsc::Receiver<PingResult>,
        collector_uuid: String,
        token: String,
    ) -> Self {
        Self {
            client,
            ping_results_rx,
            collector_uuid,
            token,
            buffers: HashMap::new(),
            last_acked_nanos: HashMap::new(),
        }
    }

    /// Runs the batch submitter's main event loop.
    ///
    /// Buffers incoming ping results and periodically sends batches to the database.
    /// Ensures memory bounds and handles retries on failures.
    pub async fn run(mut self) -> Result<()> {
        const BUFFER_LIMIT: usize = 1_000_000;
        let mut interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                Some(ping_result) = self.ping_results_rx.recv() => {
                    let total_buffered: usize = self.buffers.values().map(|v| v.len()).sum();
                    if total_buffered >= BUFFER_LIMIT
                        && let Some((target, buffer)) = self.buffers.iter_mut().max_by_key(|(_, v)| v.len()) {
                            warn!("Buffer limit reached. Dropping oldest record for target {target}");
                            buffer.pop_front();
                        }

                    let record = RawDataRecord {
                        sent_nanos: ping_result.sent_nanos,
                        rtt_nanos: ping_result.rtt.map_or(u64::MAX, |rtt| rtt.as_nanos() as u64),
                    };
                    self.buffers.entry(ping_result.target).or_default().push_back(record);
                }
                _ = interval.tick() => {
                    self.send_batches().await;
                }
            }
        }
    }

    /// Sends buffered batches to the database for all targets.
    ///
    /// Implements the ACK/DESYNC protocol to ensure eventual consistency.
    /// Retries on desyncs and logs errors for later retries.
    async fn send_batches(&mut self) {
        for (target, buffer) in self.buffers.iter_mut() {
            if buffer.is_empty() {
                continue;
            }

            let mut needs_immediate_retry = true;
            while needs_immediate_retry {
                needs_immediate_retry = false;

                if buffer.is_empty() {
                    break;
                }

                let records_to_send: Vec<_> = buffer.iter().cloned().collect();
                let last_acked = *self.last_acked_nanos.get(target).unwrap_or(&0);
                let mut request = tonic::Request::new(SendBatchRequest {
                    collector_uuid: self.collector_uuid.clone(),
                    target_ip: target.to_string(),
                    records: records_to_send,
                    collector_believes_last_acked_nanos: last_acked,
                });
                request.metadata_mut().insert(
                    "authorization",
                    format!("Bearer {}", self.token).parse().unwrap(),
                );

                let client = self.client.lock().await;
                match client.send_batch(request).await {
                    Ok(response) => {
                        match send_batch_response::Status::try_from(
                            response.status,
                        ) {
                            Ok(send_batch_response::Status::Ok) => {
                                let new_acked = response.database_confirms_last_acked_nanos;
                                info!(
                                    "Batch for target {target} sent successfully. New acked_nanos: {new_acked}"
                                );
                                self.last_acked_nanos.insert(*target, new_acked);
                                buffer.retain(|r| r.sent_nanos > new_acked);
                            }
                            Ok(send_batch_response::Status::Desync) => {
                                let new_acked = response.database_confirms_last_acked_nanos;
                                warn!(
                                    "Received DESYNC for target {target}. DB confirms acked_nanos: {new_acked}. Rewinding buffer."
                                );
                                self.last_acked_nanos.insert(*target, new_acked);
                                buffer.retain(|r| r.sent_nanos > new_acked);
                                needs_immediate_retry = true;
                            }
                            Err(_) => {
                                error!(
                                    "Unknown status for target {target} in SendBatchResponse: {}",
                                    response.status
                                );
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            "send_batch RPC for target {target} failed: {e}. Data will be retried."
                        );
                        break;
                    }
                }
            }
        }
    }
}
