//! Actor implementation for the MemDB component.
//!
//! This module contains the MemDBActor which handles both Collector and Database roles
//! for in-memory ping result storage and querying.

use crate::config::MemDBConfig;
use crate::events::MemDBEvent;
use crate::internal_messages::{
    InboundBatchAck, InboundQuery, InboundQueryResponse, InboundSubmitBatch,
};
use crate::messages::{
    ClearBuffer, GetHealth, GetStats, MemDBError, MemDBHealth, StorePingResult, TargetStats,
};
use crate::network_messages::PingResult;
use crate::storage::StorageBackend;
use actix::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;

/// The MemDBActor handles ping result storage and querying.
///
/// This actor operates in two modes:
/// - **Collector mode**: Buffers ping results and sends batches to Database peers
/// - **Database mode**: Receives batches, stores data, and provides query interface
pub struct MemDBActor {
    /// Configuration for the actor (determines behavior and capabilities)
    config: MemDBConfig,

    /// Storage backend for Database mode (None for Collector)
    storage: Option<StorageBackend>,

    /// Buffer for Collector role (unsent results)
    buffer: Vec<PingResult>,

    /// Health counters for operational visibility
    successful_batches: Arc<AtomicU64>,
    failed_batches: Arc<AtomicU64>,
    total_results: Arc<AtomicU64>,

    /// For Collector role: track the timestamp of the currently outstanding batch
    outstanding_batch: Option<u64>,

    /// Event bus for broadcasting outbound events to NetworkActors
    event_tx: broadcast::Sender<MemDBEvent>,
}

impl Default for MemDBActor {
    fn default() -> Self {
        Self::new(MemDBConfig::default())
    }
}

impl MemDBActor {
    /// Create a new MemDBActor with configuration
    pub fn new(config: MemDBConfig) -> Self {
        if let Err(e) = config.validate() {
            panic!("Invalid configuration: {}", e);
        }

        let storage = if config.accept_batches {
            Some(StorageBackend::new(config.max_results_per_target))
        } else {
            None
        };

        let (event_tx, _) = broadcast::channel(100);

        Self {
            config,
            storage,
            buffer: Vec::new(),
            successful_batches: Arc::new(AtomicU64::new(0)),
            failed_batches: Arc::new(AtomicU64::new(0)),
            total_results: Arc::new(AtomicU64::new(0)),
            outstanding_batch: None,
            event_tx,
        }
    }

    /// Get the event bus sender so NetworkActors can subscribe.
    pub fn event_bus(&self) -> broadcast::Sender<MemDBEvent> {
        self.event_tx.clone()
    }

    /// Store a ping result (used by both roles)
    fn store_result(&mut self, result: PingResult) {
        if let Some(storage) = &mut self.storage {
            let batch_timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            storage.insert_batch(vec![result], batch_timestamp);
        } else {
            // This shouldn't happen in normal operation, but handle gracefully
            log::warn!("Attempted to store result but no storage backend available");
        }

        self.total_results.fetch_add(1, Ordering::Relaxed);
    }

    /// Send a batch of results to Database peers (Collector mode only)
    fn send_batch(&mut self, _ctx: &mut Context<Self>) -> Result<(), MemDBError> {
        if self.config.accept_batches {
            return Err(MemDBError::WrongRole);
        }

        if self.buffer.is_empty() {
            return Ok(()); // Nothing to send
        }

        if self.outstanding_batch.is_some() {
            log::warn!("Already have outstanding batch, not sending new one");
            return Ok(());
        }

        let results = std::mem::take(&mut self.buffer);
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let results_count = results.len();
        let event_payload = results.clone();

        match self.event_tx.send(MemDBEvent::BatchReady {
            timestamp_ms,
            results: event_payload,
        }) {
            Ok(subscribers) if subscribers > 0 => {
                self.outstanding_batch = Some(timestamp_ms);
                log::info!(
                    "Collector batch published (timestamp: {}, results: {}, subscribers: {})",
                    timestamp_ms,
                    results_count,
                    subscribers
                );
                self.successful_batches.fetch_add(1, Ordering::Relaxed);
            }
            Ok(_) => {
                // No live subscribers: re-buffer and signal backpressure so we retry later.
                self.buffer = results;
                log::warn!(
                    "Batch publish skipped – no subscribers available (timestamp: {})",
                    timestamp_ms
                );
                self.failed_batches.fetch_add(1, Ordering::Relaxed);
                return Err(MemDBError::NetworkError(
                    "No network subscribers available".to_string(),
                ));
            }
            Err(err) => {
                let reason = err.to_string();
                let MemDBEvent::BatchReady {
                    results: failed_results,
                    ..
                } = err.0;
                self.buffer = failed_results;
                self.failed_batches.fetch_add(1, Ordering::Relaxed);
                log::warn!(
                    "Failed to publish batch event (timestamp: {}): {}",
                    timestamp_ms,
                    reason
                );
                return Err(MemDBError::InternalError(
                    "Event bus publish failed".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Get statistics for a specific target
    fn get_target_stats(&self, target: &str) -> TargetStats {
        if let Some(storage) = &self.storage {
            let (result_count, avg_rtt_us, packet_loss_percent, last_seen_ms) =
                storage.get_target_stats(target);
            TargetStats {
                target: target.to_string(),
                result_count,
                avg_rtt_us,
                packet_loss_percent,
                last_seen_ms,
            }
        } else {
            // No storage available
            TargetStats {
                target: target.to_string(),
                result_count: 0,
                avg_rtt_us: None,
                packet_loss_percent: 0.0,
                last_seen_ms: None,
            }
        }
    }
}

impl Actor for MemDBActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Context<Self>) {
        let mode = if self.config.accept_batches {
            "database"
        } else {
            "collector"
        };
        log::info!("MemDBActor has started in {} mode", mode);
    }

    fn stopped(&mut self, _ctx: &mut Context<Self>) {
        log::info!("MemDBActor has stopped");
    }
}

// ============================================================================
// INBOUND MESSAGE HANDLERS (Network → MainActor)
// ============================================================================

/// Database receives batch from Collector
impl Handler<InboundSubmitBatch> for MemDBActor {
    type Result = Result<crate::internal_messages::BatchAckResponse, String>;

    fn handle(&mut self, msg: InboundSubmitBatch, _ctx: &mut Context<Self>) -> Self::Result {
        let received_count = msg.results.len();
        log::debug!(
            "Database received batch with {} results from peer {}",
            received_count,
            msg.peer_id
        );

        if let Some(storage) = &mut self.storage {
            storage.insert_batch(msg.results, msg.timestamp_ms);
            self.total_results
                .fetch_add(received_count as u64, Ordering::Relaxed);
            self.successful_batches.fetch_add(1, Ordering::Relaxed);
        }

        let ack_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Ok(crate::internal_messages::BatchAckResponse {
            received_count,
            timestamp_ms: ack_timestamp,
        })
    }
}

/// Database receives query from Admin
impl Handler<InboundQuery> for MemDBActor {
    type Result = Result<Vec<crate::network_messages::StoredPingResult>, String>;

    fn handle(&mut self, msg: InboundQuery, _ctx: &mut Context<Self>) -> Self::Result {
        log::debug!(
            "Database received query for target {} from peer {}",
            msg.target,
            msg.peer_id
        );

        let results = if let Some(storage) = &self.storage {
            storage.query_target(&msg.target, msg.from_ms, msg.to_ms)
        } else {
            Vec::new()
        };

        // Return results directly (no NetworkManager needed)
        Ok(results)
    }
}

/// Collector receives acknowledgment from Database
impl Handler<InboundBatchAck> for MemDBActor {
    type Result = ();

    fn handle(&mut self, msg: InboundBatchAck, _ctx: &mut Context<Self>) {
        log::debug!(
            "Collector received BatchAck for {} results at {} from peer {}",
            msg.received_count,
            msg.timestamp_ms,
            msg.peer_id
        );

        // Handle acknowledgment: clear outstanding batch and update metrics
        if let Some(batch_ts) = self.outstanding_batch.take() {
            if batch_ts == msg.timestamp_ms {
                self.successful_batches.fetch_add(1, Ordering::Relaxed);
                log::debug!(
                    "Cleared outstanding batch with timestamp {}",
                    msg.timestamp_ms
                );
            } else {
                log::warn!(
                    "BatchAck timestamp mismatch: expected {}, got {}",
                    batch_ts,
                    msg.timestamp_ms
                );
                // Put it back since it didn't match
                self.outstanding_batch = Some(batch_ts);
            }
        } else {
            log::warn!("Received BatchAck but no outstanding batch");
        }
    }
}

/// Admin receives query response from Database (future use)
impl Handler<InboundQueryResponse> for MemDBActor {
    type Result = ();

    fn handle(&mut self, msg: InboundQueryResponse, _ctx: &mut Context<Self>) {
        log::debug!(
            "Received QueryResponse with {} results from peer {}",
            msg.results.len(),
            msg.peer_id
        );
        // NOTE: Future implementation - forward to UI or store for admin interface
    }
}

impl Handler<StorePingResult> for MemDBActor {
    type Result = Result<(), MemDBError>;

    fn handle(&mut self, msg: StorePingResult, ctx: &mut Context<Self>) -> Self::Result {
        if !self.config.accept_batches {
            // Collector mode: buffer the result
            let target = msg.result.target.clone();
            self.buffer.push(msg.result);
            log::debug!("Buffered ping result for {}", target);

            // Check if we should send a batch
            if self.config.buffer_size > 0 && self.buffer.len() >= self.config.buffer_size {
                self.send_batch(ctx)?;
            }
        } else {
            // Database mode: store directly
            self.store_result(msg.result);
        }

        Ok(())
    }
}

impl Handler<ClearBuffer> for MemDBActor {
    type Result = Result<(), MemDBError>;

    fn handle(&mut self, _msg: ClearBuffer, _ctx: &mut Context<Self>) -> Self::Result {
        if self.config.accept_batches {
            return Err(MemDBError::WrongRole);
        }

        let cleared_count = self.buffer.len();
        self.buffer.clear();
        log::info!("Cleared buffer of {} results", cleared_count);

        Ok(())
    }
}

impl Handler<GetHealth> for MemDBActor {
    type Result = Result<MemDBHealth, MemDBError>;

    fn handle(&mut self, _msg: GetHealth, _ctx: &mut Context<Self>) -> Self::Result {
        let role_str = if self.config.accept_batches {
            "database".to_string()
        } else {
            "collector".to_string()
        };

        let buffer_size = self.buffer.len();
        let total_results = self.total_results.load(Ordering::Relaxed);
        let successful_batches = self.successful_batches.load(Ordering::Relaxed);
        let failed_batches = self.failed_batches.load(Ordering::Relaxed);

        Ok(MemDBHealth {
            role: role_str,
            buffer_size,
            total_results,
            successful_batches,
            failed_batches,
            last_batch_ms: None, // NOTE: Could track last batch timestamp in future
        })
    }
}

impl Handler<GetStats> for MemDBActor {
    type Result = Result<TargetStats, MemDBError>;

    fn handle(&mut self, msg: GetStats, _ctx: &mut Context<Self>) -> Self::Result {
        if !self.config.accept_batches {
            return Err(MemDBError::WrongRole);
        }

        Ok(self.get_target_stats(&msg.target))
    }
}
