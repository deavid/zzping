//! The core actor implementation for the `zzcollector-state` component.
//!
//! This is the MainActor in the three-actor pattern. It contains ONLY business logic:
//! - Collector state tracking
//! - Database collector registry
//! - Health metrics and counters
//! - Timer-based operations
//!
//! Network concerns are handled by:
//! - GenericNetworkManager (peer lifecycle orchestration)
//! - CStateNetworkActor (per-peer protocol translation)

use crate::{
    config::CStateConfig,
    events::CStateEvent,
    internal_messages::{
        InboundCollectorList, InboundHeartbeat, InboundHeartbeatAck, InboundQueryCollectors,
        InboundRegistrationRejected, InboundUnauthorized,
    },
    messages::{
        CStateError, CStateHealth, ForceHeartbeat, GetCollectorState, GetHealth, SetPinger,
        SetTcpLock, UpdateHealthMetrics,
    },
    state::{CollectorStateData, DatabaseStateData, TrackedCollector},
};
use actix::prelude::*;
use log::{debug, info, warn};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio_stream::wrappers::IntervalStream;
use zzpinger::UpdateCState;
use zztcp_lock::messages::{SetLockDesired, UpdateLockStatus};

/// The main actor for the `zzcollector-state` component.
///
/// This is the MainActor in the three-actor pattern. It contains ONLY business logic:
/// - Collector state management (CollectorStateData)
/// - Database collector registry (DatabaseStateData)
/// - Health counters (heartbeats_sent, heartbeats_acked, heartbeats_failed)
/// - Timer-based operations (heartbeat interval, stale cleanup)
///
/// Network communication is delegated to CStateNetworkManager.
pub struct CStateActor {
    /// The configured operational configuration for this actor instance.
    config: CStateConfig,

    /// Collector state (populated when role is Collector).
    collector_state: Option<CollectorStateData>,

    /// Database state (populated when role is Database).
    database_state: Option<DatabaseStateData>,

    /// Health counter: total heartbeats sent.
    heartbeats_sent: Arc<AtomicU64>,

    /// Health counter: total heartbeats acknowledged.
    heartbeats_acked: Arc<AtomicU64>,

    /// Health counter: total heartbeats that failed to send.
    heartbeats_failed: Arc<AtomicU64>,

    /// Event bus for broadcasting heartbeat events to all NetworkActors
    event_tx: tokio::sync::broadcast::Sender<CStateEvent>,

    /// Recipient for sending control messages to the Pinger actor.
    pinger: Option<Recipient<UpdateCState>>,

    /// Recipient for sending control messages to the TcpLock actor.
    tcp_lock: Option<Recipient<SetLockDesired>>,

    /// Tracks the last known state of the pinger.
    pinger_is_active: bool,
}

impl CStateActor {
    /// Creates a new `CStateActor`.
    ///
    /// The role determines which internal state is populated (collector or database).
    /// The network_manager will be set later via SetNetworkManager message.
    pub fn new(config: CStateConfig) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(100);
        let mut actor = Self {
            config,
            collector_state: None,
            database_state: None,
            heartbeats_sent: Arc::new(AtomicU64::new(0)),
            heartbeats_acked: Arc::new(AtomicU64::new(0)),
            heartbeats_failed: Arc::new(AtomicU64::new(0)),
            event_tx,
            pinger: None,
            tcp_lock: None,
            pinger_is_active: false,
        };
        if let Some(collector_id) = &actor.config.collector_id {
            actor.collector_state = Some(CollectorStateData::new(collector_id.clone()));
        }

        if actor.config.track_collectors {
            actor.database_state = Some(DatabaseStateData {
                stale_timeout_ms: actor.config.stale_timeout_ms,
                max_collectors: actor.config.max_collectors,
                ..Default::default()
            });
        }

        actor
    }

    /// Get event bus for NetworkActor subscriptions
    pub fn event_bus(&self) -> tokio::sync::broadcast::Sender<CStateEvent> {
        self.event_tx.clone()
    }

    /// Sends a heartbeat by notifying the NetworkManager to broadcast.
    ///
    /// This triggers the NetworkManager to send the heartbeat to all connected peers.
    fn send_heartbeat(&mut self, _ctx: &mut Context<Self>) -> Result<(), CStateError> {
        if let Some(state) = &mut self.collector_state {
            let event = CStateEvent::HeartbeatTick {
                collector_id: state.collector_id.clone(),
                uptime_secs: state.start_time.elapsed().as_secs(),
                pings_sent: state.pings_sent,
                pings_received: state.pings_received,
                batches_sent: state.batches_sent,
                last_config_update_ms: state.last_config_update_ms,
                connection_nonce: state.connection_nonce,
            };

            if let Err(e) = self.event_tx.send(event) {
                warn!("Failed to publish heartbeat event: {}", e);
                return Err(CStateError::NotConnected);
            }

            self.heartbeats_sent.fetch_add(1, Ordering::Relaxed);
            state.last_heartbeat_sent_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
        }
        Ok(())
    }

    /// Evaluates the mastership status and enables/disables the pinger accordingly.
    /// This is the critical logic that combines local lock and database authorization.
    fn evaluate_mastership(&mut self) {
        if let Some(state) = &self.collector_state {
            // A collector should desire the lock if it is authorized by the database.
            if let Some(tcp_lock) = &self.tcp_lock {
                tcp_lock.do_send(SetLockDesired {
                    required: state.database_authorized,
                });
            } else {
                warn!("Cannot update tcp_lock state: TcpLock recipient not set.");
            }

            let should_be_active = state.has_local_lock && state.database_authorized;

            if self.pinger_is_active != should_be_active {
                info!(
                    "Mastership status changed. Pinger active: {} -> {}",
                    self.pinger_is_active, should_be_active
                );
                self.pinger_is_active = should_be_active;

                if let Some(pinger) = &self.pinger {
                    pinger.do_send(UpdateCState {
                        enable: should_be_active,
                    });
                } else {
                    warn!("Cannot update pinger state: Pinger recipient not set.");
                }
            }
        }
    }
}

impl Actor for CStateActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("CStateActor started");

        // Initial evaluation of mastership
        self.evaluate_mastership();

        if let Some(heartbeat_interval_ms) = self
            .config
            .collector_id
            .as_ref()
            .map(|_| self.config.heartbeat_interval_ms)
            && heartbeat_interval_ms > 0
        {
            let interval = Duration::from_millis(heartbeat_interval_ms);
            ctx.add_stream(IntervalStream::new(tokio::time::interval(interval)));
        }

        if self.config.track_collectors {
            // check interval = half of stale timeout, minimum 1ms
            let check_ms = std::cmp::max(1, self.config.stale_timeout_ms / 2);
            let interval = Duration::from_millis(check_ms);
            ctx.run_interval(interval, |_act, ctx| {
                ctx.address()
                    .do_send(crate::messages::CleanupStaleCollectors);
            });
        }
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("CStateActor stopped");
    }
}

impl StreamHandler<tokio::time::Instant> for CStateActor {
    fn handle(&mut self, _item: tokio::time::Instant, ctx: &mut Context<Self>) {
        if let Err(e) = self.send_heartbeat(ctx) {
            warn!("Failed to send heartbeat: {}", e);
            self.heartbeats_failed.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Handler<ForceHeartbeat> for CStateActor {
    type Result = Result<(), CStateError>;

    fn handle(&mut self, _msg: ForceHeartbeat, ctx: &mut Context<Self>) -> Self::Result {
        self.send_heartbeat(ctx)
    }
}

// ============================================================================
// Inbound Message Handlers (NetworkActor → MainActor)
// ============================================================================

impl Handler<crate::internal_messages::InboundPrepareToSwap> for CStateActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::internal_messages::InboundPrepareToSwap,
        _ctx: &mut Context<Self>,
    ) {
        if self.config.collector_id.is_some() {
            info!(
                "Received PrepareToSwap from peer {:?} for time {}",
                msg.peer_id, msg.swap_time_ms
            );
            // In the future, this could trigger buffer flushing.
        }
    }
}

impl Handler<crate::internal_messages::InboundSetMastership> for CStateActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::internal_messages::InboundSetMastership,
        _ctx: &mut Context<Self>,
    ) {
        if let Some(state) = &mut self.collector_state {
            info!(
                "Received SetMastership from peer {:?}: is_primary={}",
                msg.peer_id, msg.is_primary
            );
            state.database_authorized = msg.is_primary;
            self.evaluate_mastership();
        }
    }
}

use crate::internal_messages::HandoffOrder;
use crate::network_messages::CStateMessage;

impl Handler<HandoffOrder> for CStateActor {
    type Result = ();

    fn handle(&mut self, msg: HandoffOrder, _ctx: &mut Context<Self>) {
        info!("Executing handoff for collector ID: {}", msg.collector_id);

        // 1. Tell the old collector to release the lock and become standby.
        msg.old_recipient
            .do_send(CStateMessage::SetMastership { is_primary: false });

        // 2. Tell the new collector to acquire the lock and become primary.
        msg.new_recipient
            .do_send(CStateMessage::SetMastership { is_primary: true });

        // 3. Update the registry to point to the new collector's recipient and nonce.
        if let Some(state) = &mut self.database_state
            && let Some(collector) = state.collectors.get_mut(&msg.collector_id)
        {
            collector.recipient = Some(msg.new_recipient);
            collector.connection_nonce = msg.new_nonce;
            info!(
                "Updated collector registry for {} to new nonce {}",
                msg.collector_id, msg.new_nonce
            );
        }
    }
}

impl Handler<InboundHeartbeat> for CStateActor {
    type Result = Result<crate::internal_messages::HeartbeatAckResponse, String>;

    fn handle(&mut self, msg: InboundHeartbeat, ctx: &mut Context<Self>) -> Self::Result {
        if !self.config.track_collectors {
            return Err("Not configured to track collectors".to_string());
        }

        let state = self
            .database_state
            .as_mut()
            .ok_or("Database state not initialized")?;

        debug!("Received heartbeat from collector: {}", msg.collector_id);

        // --- Handoff Logic ---
        if let Some(existing_collector) = state.collectors.get(&msg.collector_id)
            && existing_collector.connection_nonce != msg.connection_nonce
        {
            info!(
                "Handoff detected for collector ID: {}. Old nonce: {}, New nonce: {}",
                msg.collector_id, existing_collector.connection_nonce, msg.connection_nonce
            );

            if let Some(old_recipient) = existing_collector.recipient.clone() {
                let swap_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    + Duration::from_secs(5);

                // 1. Tell the old collector to prepare for the swap.
                old_recipient.do_send(CStateMessage::PrepareToSwap {
                    swap_time_ms: swap_time.as_millis() as u64,
                });

                // First, update the heartbeat to prevent stale cleanup before the handoff.
                if let Some(collector) = state.collectors.get_mut(&msg.collector_id) {
                    collector.update_heartbeat(
                        msg.uptime_secs,
                        msg.pings_sent,
                        msg.pings_received,
                        msg.batches_sent,
                        msg.last_config_update_ms,
                    );
                }

                // Then, schedule the actual swap to happen in 5 seconds.
                ctx.run_later(Duration::from_secs(5), move |_act, ctx| {
                    ctx.address().do_send(HandoffOrder {
                        collector_id: msg.collector_id,
                        old_recipient,
                        new_recipient: msg.recipient,
                        new_nonce: msg.connection_nonce,
                    });
                });

                // Acknowledge the heartbeat immediately but don't update the registry.
                let timestamp_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                return Ok(crate::internal_messages::HeartbeatAckResponse {
                    timestamp_ms,
                    server_time_ms: timestamp_ms,
                    rejection: None,
                });
            }
        }
        // --- End Handoff Logic ---

        if let Some(max) = state.max_collectors
            && !state.collectors.contains_key(&msg.collector_id)
            && state.collectors.len() >= max
        {
            let timestamp_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            return Ok(crate::internal_messages::HeartbeatAckResponse {
                timestamp_ms,
                server_time_ms: timestamp_ms,
                rejection: Some(format!("Database at capacity (max {})", max)),
            });
        }

        let collector = state
            .collectors
            .entry(msg.collector_id.clone())
            .or_insert_with(|| {
                info!("Registered new collector: {}", msg.collector_id);
                TrackedCollector::new(msg.collector_id.clone(), msg.connection_nonce)
            });

        // This is a new collector or a heartbeat from the existing primary. Update everything.
        collector.recipient = Some(msg.recipient);
        collector.connection_nonce = msg.connection_nonce;
        collector.update_heartbeat(
            msg.uptime_secs,
            msg.pings_sent,
            msg.pings_received,
            msg.batches_sent,
            msg.last_config_update_ms,
        );

        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(crate::internal_messages::HeartbeatAckResponse {
            timestamp_ms,
            server_time_ms: timestamp_ms,
            rejection: None,
        })
    }
}

impl Handler<InboundHeartbeatAck> for CStateActor {
    type Result = ();

    fn handle(&mut self, msg: InboundHeartbeatAck, _ctx: &mut Context<Self>) {
        // Only collector-configured instances process acks
        if self.config.collector_id.is_none() {
            return;
        }

        if let Some(state) = &mut self.collector_state {
            debug!(
                "Received heartbeat ack: timestamp={}, server_time={}",
                msg.timestamp_ms, msg.server_time_ms
            );
            self.heartbeats_acked.fetch_add(1, Ordering::Relaxed);
            state.last_heartbeat_ack_ms = msg.timestamp_ms;
        }
    }
}

impl Handler<InboundQueryCollectors> for CStateActor {
    type Result = Result<Vec<crate::network_messages::CollectorInfo>, String>;

    fn handle(&mut self, msg: InboundQueryCollectors, _ctx: &mut Context<Self>) -> Self::Result {
        // Only database-like configurations can respond to queries
        if !self.config.track_collectors {
            return Err("Not configured to track collectors".to_string());
        }

        if let Some(state) = &self.database_state {
            debug!("Received collector list query from peer: {:?}", msg.peer_id);

            let collectors: Vec<crate::network_messages::CollectorInfo> = state
                .collectors
                .values()
                .map(|c| crate::network_messages::CollectorInfo {
                    id: c.id.clone(),
                    last_seen_ms: c.last_seen_ms,
                    uptime_secs: c.uptime_secs,
                    pings_sent: c.pings_sent,
                    pings_received: c.pings_received,
                    connection_nonce: c.connection_nonce,
                })
                .collect();

            Ok(collectors)
        } else {
            Err("Database state not initialized".to_string())
        }
    }
}

impl Handler<InboundCollectorList> for CStateActor {
    type Result = ();

    fn handle(&mut self, msg: InboundCollectorList, _ctx: &mut Context<Self>) {
        // Collector or Admin roles can receive collector lists
        debug!(
            "Received collector list from peer {:?}: {} collectors",
            msg.peer_id,
            msg.collectors.len()
        );
        // For now, just log. Future: update local cache or display in UI
    }
}

impl Handler<InboundRegistrationRejected> for CStateActor {
    type Result = ();

    fn handle(&mut self, msg: InboundRegistrationRejected, _ctx: &mut Context<Self>) {
        warn!(
            "Registration rejected by peer {:?}: {}",
            msg.peer_id, msg.reason
        );
        // TODO: Implement retry logic or notify application layer
    }
}

impl Handler<InboundUnauthorized> for CStateActor {
    type Result = ();

    fn handle(&mut self, msg: InboundUnauthorized, _ctx: &mut Context<Self>) {
        warn!(
            "Unauthorized response from peer {:?}: {}",
            msg.peer_id, msg.reason
        );
        // TODO: Implement error handling or notify application layer
    }
}

// ============================================================================
// Message Handlers
// ============================================================================

impl Handler<UpdateLockStatus> for CStateActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateLockStatus, _ctx: &mut Context<Self>) {
        if let Some(state) = &mut self.collector_state
            && state.has_local_lock != msg.locked
        {
            debug!("Lock status updated to: {}", msg.locked);
            state.has_local_lock = msg.locked;
            self.evaluate_mastership();
        }
    }
}

impl Handler<SetPinger> for CStateActor {
    type Result = ();

    fn handle(&mut self, msg: SetPinger, _ctx: &mut Context<Self>) {
        info!("Pinger recipient has been set.");
        self.pinger = Some(msg.pinger);
        // Re-evaluate mastership now that we have a pinger to control
        self.evaluate_mastership();
    }
}

impl Handler<SetTcpLock> for CStateActor {
    type Result = ();

    fn handle(&mut self, msg: SetTcpLock, _ctx: &mut Context<Self>) {
        info!("TcpLock recipient has been set.");
        self.tcp_lock = Some(msg.tcp_lock);
        // Re-evaluate mastership now that we have a tcp_lock to control
        self.evaluate_mastership();
    }
}

impl Handler<crate::messages::CleanupStaleCollectors> for CStateActor {
    type Result = ();

    fn handle(&mut self, _msg: crate::messages::CleanupStaleCollectors, _ctx: &mut Context<Self>) {
        if let Some(state) = &mut self.database_state {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            let stale_timeout_ms = state.stale_timeout_ms;
            let mut to_remove = Vec::new();

            for (id, collector) in &state.collectors {
                if now.saturating_sub(collector.last_seen_ms) > stale_timeout_ms {
                    to_remove.push(id.clone());
                }
            }

            for id in to_remove {
                info!("Removing stale collector: {}", id);
                state.collectors.remove(&id);
            }
        }
    }
}

impl Handler<UpdateHealthMetrics> for CStateActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateHealthMetrics, _ctx: &mut Context<Self>) {
        if let Some(state) = &mut self.collector_state {
            if let Some(pings_sent) = msg.pings_sent {
                state.pings_sent = pings_sent;
            }
            if let Some(pings_received) = msg.pings_received {
                state.pings_received = pings_received;
            }
            if let Some(batches_sent) = msg.batches_sent {
                state.batches_sent = batches_sent;
            }
            if let Some(last_config_update_ms) = msg.last_config_update_ms {
                state.last_config_update_ms = last_config_update_ms;
            }
        }
    }
}

impl Handler<GetHealth> for CStateActor {
    type Result = Result<CStateHealth, CStateError>;

    fn handle(&mut self, _msg: GetHealth, _ctx: &mut Context<Self>) -> Self::Result {
        Ok(CStateHealth {
            heartbeats_sent: self.heartbeats_sent.load(Ordering::Relaxed),
            heartbeats_acked: self.heartbeats_acked.load(Ordering::Relaxed),
            heartbeats_failed: self.heartbeats_failed.load(Ordering::Relaxed),
        })
    }
}

impl Handler<GetCollectorState> for CStateActor {
    type Result = Result<CollectorStateData, CStateError>;

    fn handle(&mut self, _msg: GetCollectorState, _ctx: &mut Context<Self>) -> Self::Result {
        self.collector_state
            .clone()
            .ok_or_else(|| CStateError::InvalidRole("Collector".to_string()))
    }
}
