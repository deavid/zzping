//! The core actor implementation for the `zzcollector-state` component.

use crate::{
    messages::{
        CStateError, CStateHealth, ForceHeartbeat, GetCollectorState, GetHealth,
        UpdateHealthMetrics, WrappedCStateMessage,
    },
    network_messages::CStateMessage,
    role::CStateRole,
    state::{CollectorStateData, DatabaseStateData, TrackedCollector},
};
use actix::prelude::*;
use log::{debug, info, warn};
use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio_stream::wrappers::IntervalStream;
use zznet_auth::ApplicationRole;
use zznet_room::room::{Room, RoomChannels};

/// The main actor for the `zzcollector-state` component.
///
/// This actor manages the state of a collector instance, including its identity,
/// health, and registration with a database. It can be configured to run in
/// one of three roles: `Collector`, `Database`, or `Admin`.
pub struct CStateActor<TRole>
where
    TRole: ApplicationRole,
{
    role: CStateRole,
    collector_state: Option<CollectorStateData>,
    database_state: Option<DatabaseStateData>,
    // health counters
    heartbeats_sent: Arc<AtomicU64>,
    heartbeats_acked: Arc<AtomicU64>,
    heartbeats_failed: Arc<AtomicU64>,

    /// Room instance for component-to-component messaging
    room: Option<Room<CStateMessage>>,

    /// Room channels for SessionManager wiring
    room_channels: Option<Arc<RoomChannels>>,

    _phantom: PhantomData<TRole>,
}

impl<TRole> CStateActor<TRole>
where
    TRole: ApplicationRole,
{
    /// Creates a new `CStateActor`.
    pub fn new(role: CStateRole) -> Self {
        let mut actor = Self {
            role,
            collector_state: None,
            database_state: None,
            heartbeats_sent: Arc::new(AtomicU64::new(0)),
            heartbeats_acked: Arc::new(AtomicU64::new(0)),
            heartbeats_failed: Arc::new(AtomicU64::new(0)),
            room: None,
            room_channels: None,
            _phantom: PhantomData,
        };

        match &actor.role {
            CStateRole::Collector { collector_id, .. } => {
                actor.collector_state = Some(CollectorStateData::new(collector_id.clone()));
            }
            CStateRole::Database {
                stale_timeout_secs,
                max_collectors,
                ..
            } => {
                let state = DatabaseStateData {
                    stale_timeout_secs: *stale_timeout_secs,
                    max_collectors: *max_collectors,
                    ..Default::default()
                };
                actor.database_state = Some(state);
            }
            CStateRole::Admin => {}
        }

        actor
    }

    /// Sets the room for this actor.
    pub fn with_room(mut self, room: Room<CStateMessage>) -> Self {
        self.room = Some(room);
        self
    }

    /// Sets the room channels for this actor.
    pub fn with_room_channels(mut self, channels: Arc<RoomChannels>) -> Self {
        self.room_channels = Some(channels);
        self
    }

    /// Sends a heartbeat message using the room.
    fn send_heartbeat(&mut self, _ctx: &mut Context<Self>) -> Result<(), CStateError> {
        if let CStateRole::Collector { collector_id, .. } = &self.role {
            if let Some(room) = &self.room {
                if let Some(state) = &mut self.collector_state {
                    let msg = CStateMessage::Heartbeat {
                        collector_id: collector_id.clone(),
                        uptime_secs: state.start_time.elapsed().as_secs(),
                        pings_sent: state.pings_sent,
                        pings_received: state.pings_received,
                        batches_sent: state.batches_sent,
                        last_config_update_ms: state.last_config_update_ms,
                        connection_nonce: state.connection_nonce,
                    };

                    let sender = room.sender();
                    actix::spawn(async move {
                        let bytes = match bincode::serde::encode_to_vec(
                            &msg,
                            bincode::config::standard(),
                        ) {
                            Ok(b) => b,
                            Err(e) => {
                                warn!("Failed to serialize heartbeat: {}", e);
                                return;
                            }
                        };
                        if let Err(e) = sender.send(bytes).await {
                            warn!("Failed to send heartbeat: {}", e);
                        }
                    });

                    self.heartbeats_sent.fetch_add(1, Ordering::Relaxed);
                    state.last_heartbeat_sent_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                }
            } else {
                return Err(CStateError::NotConnected);
            }
        } else {
            return Err(CStateError::InvalidRole("Collector".to_string()));
        }
        Ok(())
    }
}

impl<TRole> Actor for CStateActor<TRole>
where
    TRole: ApplicationRole,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("CStateActor started in role: {:?}", self.role);

        // Start heartbeat timer for collectors
        if let CStateRole::Collector {
            heartbeat_interval_ms,
            ..
        } = self.role
        {
            let interval = Duration::from_millis(heartbeat_interval_ms);
            ctx.add_stream(IntervalStream::new(tokio::time::interval(interval)));
        }

        // If running as Database, schedule stale cleanup
        if let CStateRole::Database {
            stale_timeout_secs, ..
        } = &self.role
        {
            // check interval = half of stale timeout, minimum 1s
            let check = std::cmp::max(1, stale_timeout_secs / 2);
            let interval = Duration::from_secs(check);
            ctx.run_interval(interval, |_act, ctx| {
                ctx.address()
                    .do_send(crate::messages::CleanupStaleCollectors);
            });
        }
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("CStateActor stopped in role: {:?}", self.role);
    }
}

impl<TRole> StreamHandler<tokio::time::Instant> for CStateActor<TRole>
where
    TRole: ApplicationRole,
{
    fn handle(&mut self, _item: tokio::time::Instant, ctx: &mut Context<Self>) {
        if let Err(e) = self.send_heartbeat(ctx) {
            warn!("Failed to send heartbeat: {}", e);
            self.heartbeats_failed.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl<TRole> Handler<ForceHeartbeat> for CStateActor<TRole>
where
    TRole: ApplicationRole,
{
    type Result = Result<(), CStateError>;

    fn handle(&mut self, _msg: ForceHeartbeat, ctx: &mut Context<Self>) -> Self::Result {
        self.send_heartbeat(ctx)
    }
}

impl<TRole> Handler<WrappedCStateMessage> for CStateActor<TRole>
where
    TRole: ApplicationRole,
{
    type Result = ();

    fn handle(&mut self, msg: WrappedCStateMessage, _ctx: &mut Context<Self>) {
        match &self.role {
            CStateRole::Database { .. } => {
                if let Some(state) = &mut self.database_state {
                    match msg.message {
                        CStateMessage::Heartbeat {
                            collector_id,
                            uptime_secs,
                            pings_sent,
                            pings_received,
                            batches_sent,
                            connection_nonce,
                            last_config_update_ms,
                        } => {
                            debug!("Received heartbeat from collector: {}", collector_id);
                            // Enforce max_collectors policy if configured: reject new registrations
                            let mut reject = false;
                            if let Some(max) = state.max_collectors
                                && state.collectors.len() >= max
                            {
                                reject = true;
                            }

                            if reject {
                                // Send rejection message back to collector
                                if let Some(room) = &self.room {
                                    let sender = room.sender();
                                    let rejection = CStateMessage::RegistrationRejected {
                                        reason: format!(
                                            "Database at capacity (max {})",
                                            state.max_collectors.unwrap()
                                        ),
                                    };
                                    actix::spawn(async move {
                                        if let Ok(bytes) = bincode::serde::encode_to_vec(
                                            &rejection,
                                            bincode::config::standard(),
                                        ) {
                                            let _ = sender.send(bytes).await;
                                        }
                                    });
                                }
                            } else {
                                // Register or update collector
                                let collector = state
                                    .collectors
                                    .entry(collector_id.clone())
                                    .or_insert_with(|| {
                                        info!("Registered new collector: {}", collector_id);
                                        TrackedCollector::new(
                                            collector_id.clone(),
                                            connection_nonce,
                                        )
                                    });

                                collector.update_heartbeat(
                                    uptime_secs,
                                    pings_sent,
                                    pings_received,
                                    batches_sent,
                                    last_config_update_ms,
                                );

                                // Send acknowledgment
                                if let Some(room) = &self.room {
                                    let sender = room.sender();
                                    let ack = CStateMessage::HeartbeatAck {
                                        timestamp_ms: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis()
                                            as u64,
                                        server_time_ms: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis()
                                            as u64,
                                    };
                                    actix::spawn(async move {
                                        if let Ok(bytes) = bincode::serde::encode_to_vec(
                                            &ack,
                                            bincode::config::standard(),
                                        ) {
                                            let _ = sender.send(bytes).await;
                                        }
                                    });
                                }
                            }
                        }
                        CStateMessage::QueryCollectors => {
                            debug!("Received collector list query");
                            if let Some(room) = &self.room {
                                let sender = room.sender();
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

                                let response = CStateMessage::CollectorList { collectors };
                                actix::spawn(async move {
                                    if let Ok(bytes) = bincode::serde::encode_to_vec(
                                        &response,
                                        bincode::config::standard(),
                                    ) {
                                        let _ = sender.send(bytes).await;
                                    }
                                });
                            }
                        }
                        _ => {
                            warn!("Database received unexpected message type");
                        }
                    }
                }
            }
            CStateRole::Collector { .. } => {
                if let Some(state) = &mut self.collector_state {
                    match msg.message {
                        CStateMessage::HeartbeatAck {
                            timestamp_ms,
                            server_time_ms,
                        } => {
                            debug!(
                                "Received heartbeat ack: timestamp={}, server_time={}",
                                timestamp_ms, server_time_ms
                            );
                            self.heartbeats_acked.fetch_add(1, Ordering::Relaxed);
                            state.last_heartbeat_ack_ms = timestamp_ms;
                        }
                        CStateMessage::RegistrationRejected { reason } => {
                            warn!("Registration rejected: {}", reason);
                        }
                        _ => {
                            warn!("Collector received unexpected message type");
                        }
                    }
                }
            }
            CStateRole::Admin => {
                // Admin can receive any messages for monitoring
                debug!("Admin received message: {:?}", msg.message);
            }
        }
    }
}

impl<TRole> Handler<crate::messages::CleanupStaleCollectors> for CStateActor<TRole>
where
    TRole: ApplicationRole,
{
    type Result = ();

    fn handle(&mut self, _msg: crate::messages::CleanupStaleCollectors, _ctx: &mut Context<Self>) {
        if let Some(state) = &mut self.database_state {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            let stale_timeout_ms = state.stale_timeout_secs * 1000;
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

impl<TRole> Handler<UpdateHealthMetrics> for CStateActor<TRole>
where
    TRole: ApplicationRole,
{
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

impl<TRole> Handler<GetHealth> for CStateActor<TRole>
where
    TRole: ApplicationRole,
{
    type Result = Result<CStateHealth, CStateError>;

    fn handle(&mut self, _msg: GetHealth, _ctx: &mut Context<Self>) -> Self::Result {
        Ok(CStateHealth {
            heartbeats_sent: self.heartbeats_sent.load(Ordering::Relaxed),
            heartbeats_acked: self.heartbeats_acked.load(Ordering::Relaxed),
            heartbeats_failed: self.heartbeats_failed.load(Ordering::Relaxed),
        })
    }
}

impl<TRole> Handler<GetCollectorState> for CStateActor<TRole>
where
    TRole: ApplicationRole,
{
    type Result = Result<CollectorStateData, CStateError>;

    fn handle(&mut self, _msg: GetCollectorState, _ctx: &mut Context<Self>) -> Self::Result {
        self.collector_state
            .clone()
            .ok_or_else(|| CStateError::InvalidRole("Collector".to_string()))
    }
}
