//! Actor implementation for the MemDB component.
//!
//! This module contains the MemDBActor which handles both Collector and Database roles
//! for in-memory ping result storage and querying.

use crate::config::MemDBConfig;
use crate::events::MemDBEvent;
use crate::internal_messages::{
    CheckOutstandingBatchTimeout, FlushToStorage, InboundBatchAck, InboundHelloCollector,
    InboundQuery, InboundQueryResponse, InboundSubmitBatch, NewCollector,
};
use crate::messages::{
    ClearBuffer, ForceFlush, GetHealth, GetStats, MemDBError, MemDBHealth, StorePingResult,
    TargetStats,
};
use crate::types::PingResult;
use actix::prelude::*;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use zzstorage::actor::StorageActor;

/// A unique identifier for a collector.
type CollectorID = String;

/// A ring buffer for storing ping results from a single collector.
type RingBuffer = VecDeque<PingResult>;

/// Timeout in seconds for retrying an outstanding batch.
const OUTSTANDING_BATCH_TIMEOUT_SECS: u64 = 3;

/// The MemDBActor handles ping result storage and querying.
pub struct MemDBActor {
    config: MemDBConfig,
    registry: HashMap<CollectorID, RingBuffer>,
    successful_batches: Arc<AtomicU64>,
    failed_batches: Arc<AtomicU64>,
    total_results: Arc<AtomicU64>,
    outstanding_batch: Option<u64>,
    outstanding_batch_buffered_count: u32,
    outstanding_batch_sent_time: Option<Instant>,
    outstanding_batch_data: Option<Vec<PingResult>>,
    event_tx: broadcast::Sender<MemDBEvent>,
    storage_actor: Option<Addr<StorageActor>>,
}

impl Default for MemDBActor {
    fn default() -> Self {
        Self::new(MemDBConfig::default())
    }
}

impl MemDBActor {
    /// Creates a new MemDBActor with configuration.
    pub fn new(config: MemDBConfig) -> Self {
        if let Err(e) = config.validate() {
            panic!("Invalid configuration: {}", e);
        }
        let (event_tx, _) = broadcast::channel(100);
        let storage_actor = if config.accept_batches {
            let storage_config = zzstorage::actor::StorageConfig::FileSystem {
                path: std::path::PathBuf::from("./"), // FIXME: Make this configurable
            };
            Some(zzstorage::actor::StorageActor::new(storage_config).start())
        } else {
            None
        };
        Self {
            config,
            registry: HashMap::new(),
            successful_batches: Arc::new(AtomicU64::new(0)),
            failed_batches: Arc::new(AtomicU64::new(0)),
            total_results: Arc::new(AtomicU64::new(0)),
            outstanding_batch: None,
            outstanding_batch_buffered_count: 0,
            outstanding_batch_sent_time: None,
            outstanding_batch_data: None,
            event_tx,
            storage_actor,
        }
    }

    /// Get the event bus sender so NetworkActors can subscribe.
    pub fn event_bus(&self) -> broadcast::Sender<MemDBEvent> {
        self.event_tx.clone()
    }

    /// (Test only) Set the storage actor address.
    pub fn set_storage_actor(&mut self, addr: Addr<StorageActor>) {
        self.storage_actor = Some(addr);
    }

    fn store_result(&mut self, result: PingResult, collector_id: &CollectorID) {
        let buffer = self.registry.entry(collector_id.clone()).or_default();
        buffer.push_back(result);
        self.total_results.fetch_add(1, Ordering::Relaxed);
    }

    fn check_and_resend_timed_out_batch(&mut self) {
        if self.config.accept_batches {
            return;
        }
        if let Some(batch_ts) = self.outstanding_batch
            && let Some(sent_time) = self.outstanding_batch_sent_time
        {
            let elapsed = sent_time.elapsed();
            let timeout_by_time = elapsed >= Duration::from_secs(OUTSTANDING_BATCH_TIMEOUT_SECS);
            let timeout_by_count = self.outstanding_batch_buffered_count >= 100;
            if timeout_by_time || timeout_by_count {
                log::warn!("Outstanding batch {} timed out. Resending.", batch_ts);
                if let Some(cached_data) = self.outstanding_batch_data.take() {
                    let new_timestamp_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let event_payload = cached_data.clone();
                    if let Ok(subscribers) = self.event_tx.send(MemDBEvent::BatchReady {
                        timestamp_ms: new_timestamp_ms,
                        results: event_payload,
                    }) {
                        if subscribers > 0 {
                            self.outstanding_batch = Some(new_timestamp_ms);
                            self.outstanding_batch_sent_time = Some(Instant::now());
                            self.outstanding_batch_data = Some(cached_data);
                        } else {
                            self.outstanding_batch_data = Some(cached_data);
                        }
                    }
                }
            }
        }
    }

    fn send_batch(&mut self, _ctx: &mut Context<Self>) -> Result<(), MemDBError> {
        self.send_batch_internal(_ctx, false)
    }

    fn send_batch_internal(
        &mut self,
        _ctx: &mut Context<Self>,
        force: bool,
    ) -> Result<(), MemDBError> {
        if self.config.accept_batches {
            return Err(MemDBError::WrongRole);
        }
        let buffer = self.registry.entry("collector".to_string()).or_default();
        if !force && buffer.is_empty() {
            return Ok(());
        }
        if !force && self.outstanding_batch.is_some() {
            return Ok(());
        }

        let results: Vec<PingResult> = buffer.drain(..).collect();
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let event_payload = results.clone();

        match self.event_tx.send(MemDBEvent::BatchReady {
            timestamp_ms,
            results: event_payload,
        }) {
            Ok(subscribers) if subscribers > 0 => {
                self.outstanding_batch = Some(timestamp_ms);
                self.outstanding_batch_buffered_count = 0;
                self.outstanding_batch_sent_time = Some(Instant::now());
                self.outstanding_batch_data = Some(results);
                self.successful_batches.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                if force {
                    // For force flush, don't put back, just mark as successful attempt
                    self.successful_batches.fetch_add(1, Ordering::Relaxed);
                } else {
                    let buffer = self.registry.entry("collector".to_string()).or_default();
                    for result in results {
                        buffer.push_back(result);
                    }
                    self.failed_batches.fetch_add(1, Ordering::Relaxed);
                    return Err(MemDBError::NetworkError(
                        "No network subscribers".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn flush_to_storage(&mut self) {
        if let Some(storage_actor) = &self.storage_actor {
            let mut batch_to_store = Vec::new();
            for buffer in self.registry.values_mut() {
                batch_to_store.extend(buffer.drain(..).map(|r| zzstorage::codec::PingResult {
                    target: r.target,
                    sent_time_ns: r.sent_time_ns,
                    status: match r.status {
                        crate::types::PingStatus::Success(ns) => {
                            zzstorage::codec::PingStatus::Success(ns)
                        }
                        crate::types::PingStatus::Timeout => zzstorage::codec::PingStatus::Timeout,
                        crate::types::PingStatus::IOError => zzstorage::codec::PingStatus::IOError,
                        crate::types::PingStatus::Partial => zzstorage::codec::PingStatus::Partial,
                        crate::types::PingStatus::Skipped => zzstorage::codec::PingStatus::Skipped,
                        crate::types::PingStatus::Other => zzstorage::codec::PingStatus::Other,
                    },
                }));
            }
            if !batch_to_store.is_empty() {
                storage_actor.do_send(zzstorage::actor::StoreBatch(batch_to_store));
            }
        }
    }

    fn get_target_stats(&self, target: &str) -> TargetStats {
        TargetStats {
            target: target.to_string(),
            result_count: 0,
            avg_rtt_us: None,
            packet_loss_percent: 0.0,
            last_seen_ms: None,
        }
    }
}

impl Actor for MemDBActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        if self.config.accept_batches {
            let flush_interval = Duration::from_secs(10);
            ctx.run_interval(flush_interval, |_, ctx| {
                ctx.address().do_send(FlushToStorage);
            });
        } else {
            let check_interval = Duration::from_millis(500);
            ctx.run_interval(check_interval, |_, ctx| {
                ctx.address().do_send(CheckOutstandingBatchTimeout);
            });
        }
    }
}

impl Handler<InboundSubmitBatch> for MemDBActor {
    type Result = Result<crate::internal_messages::BatchAckResponse, String>;
    fn handle(&mut self, msg: InboundSubmitBatch, _ctx: &mut Context<Self>) -> Self::Result {
        let received_count = msg.results.len();
        for result in msg.results {
            self.store_result(result, &msg.peer_id.to_string());
        }
        self.successful_batches.fetch_add(1, Ordering::Relaxed);
        Ok(crate::internal_messages::BatchAckResponse {
            received_count,
            timestamp_ms: msg.timestamp_ms,
        })
    }
}

impl Handler<InboundQuery> for MemDBActor {
    type Result = Result<Vec<crate::network_messages::StoredPingResult>, String>;
    fn handle(&mut self, _msg: InboundQuery, _ctx: &mut Context<Self>) -> Self::Result {
        Ok(Vec::new())
    }
}

impl Handler<InboundBatchAck> for MemDBActor {
    type Result = ();
    fn handle(&mut self, msg: InboundBatchAck, _ctx: &mut Context<Self>) {
        if let Some(batch_ts) = self.outstanding_batch.take() {
            if batch_ts == msg.timestamp_ms {
                self.successful_batches.fetch_add(1, Ordering::Relaxed);
                self.outstanding_batch_data = None;
            } else {
                self.outstanding_batch = Some(batch_ts);
            }
        }
    }
}

impl Handler<InboundQueryResponse> for MemDBActor {
    type Result = ();
    fn handle(&mut self, _msg: InboundQueryResponse, _ctx: &mut Context<Self>) {}
}

impl Handler<StorePingResult> for MemDBActor {
    type Result = Result<(), MemDBError>;
    fn handle(&mut self, msg: StorePingResult, ctx: &mut Context<Self>) -> Self::Result {
        if !self.config.accept_batches {
            let buffer = self.registry.entry("collector".to_string()).or_default();
            buffer.push_back(msg.result);
            if self.config.buffer_size > 0
                && buffer.len() >= self.config.buffer_size
                && self.send_batch(ctx).is_err()
            {
                let buffer = self.registry.entry("collector".to_string()).or_default();
                while buffer.len() > self.config.buffer_size {
                    buffer.pop_front();
                }
            }
        } else {
            // Database mode: store directly. We need a collector ID.
            // This will be handled in a new message for the rewind logic.
            // For now, we do nothing.
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
        self.registry
            .entry("collector".to_string())
            .or_default()
            .clear();
        Ok(())
    }
}

impl Handler<GetHealth> for MemDBActor {
    type Result = Result<MemDBHealth, MemDBError>;
    fn handle(&mut self, _msg: GetHealth, _ctx: &mut Context<Self>) -> Self::Result {
        Ok(MemDBHealth {
            role: if self.config.accept_batches {
                "database"
            } else {
                "collector"
            }
            .to_string(),
            buffer_size: self.registry.values().map(|b| b.len()).sum(),
            total_results: self.total_results.load(Ordering::Relaxed),
            successful_batches: self.successful_batches.load(Ordering::Relaxed),
            failed_batches: self.failed_batches.load(Ordering::Relaxed),
            last_batch_ms: None,
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

impl Handler<CheckOutstandingBatchTimeout> for MemDBActor {
    type Result = ();
    fn handle(&mut self, _msg: CheckOutstandingBatchTimeout, _ctx: &mut Self::Context) {
        self.check_and_resend_timed_out_batch();
    }
}

impl Handler<FlushToStorage> for MemDBActor {
    type Result = ();
    fn handle(&mut self, _msg: FlushToStorage, _ctx: &mut Context<Self>) {
        self.flush_to_storage();
    }
}

impl Handler<ForceFlush> for MemDBActor {
    type Result = Result<(), MemDBError>;
    fn handle(&mut self, _msg: ForceFlush, ctx: &mut Context<Self>) -> Self::Result {
        if self.config.accept_batches {
            // Database role: flush to storage
            self.flush_to_storage();
        } else {
            // Collector role: send batch
            self.send_batch_internal(ctx, true)?;
        }
        Ok(())
    }
}

impl Handler<InboundHelloCollector> for MemDBActor {
    type Result = ();
    fn handle(&mut self, msg: InboundHelloCollector, _ctx: &mut Self::Context) {
        if self.config.accept_batches {
            return;
        }

        let buffer = self.registry.entry("collector".to_string()).or_default();
        let start_index = buffer
            .iter()
            .position(|r| r.sent_time_ns > msg.last_persisted_ts)
            .unwrap_or(buffer.len());

        let to_replay: Vec<PingResult> = buffer.range(start_index..).cloned().collect();

        if !to_replay.is_empty() {
            log::info!(
                "Rewinding and replaying {} results after handshake.",
                to_replay.len()
            );
            let timestamp_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            self.event_tx
                .send(MemDBEvent::BatchReady {
                    timestamp_ms,
                    results: to_replay,
                })
                .unwrap_or_else(|e| {
                    log::error!("Failed to send replay batch: {}", e);
                    0
                });
        }
    }
}

impl Handler<NewCollector> for MemDBActor {
    type Result = ResponseFuture<()>;

    fn handle(&mut self, msg: NewCollector, _ctx: &mut Self::Context) -> Self::Result {
        if !self.config.accept_batches {
            return Box::pin(async {});
        }

        let storage_actor = self.storage_actor.clone();
        let event_tx = self.event_tx.clone();

        Box::pin(async move {
            let last_persisted_ts = if let Some(actor) = storage_actor {
                match actor
                    .send(zzstorage::actor::GetLastTimestamp {
                        target: msg.peer_id.to_string(),
                    })
                    .await
                {
                    Ok(Ok(ts)) => ts,
                    _ => 0,
                }
            } else {
                0
            };

            event_tx
                .send(MemDBEvent::HelloCollector {
                    peer_id: msg.peer_id,
                    last_persisted_ts,
                })
                .unwrap_or_else(|e| {
                    log::error!("Failed to send HelloCollector: {}", e);
                    0
                });
        })
    }
}
