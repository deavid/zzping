//! The core actor implementation for the `zzcollector-state` component.

use crate::{
    messages::{
        CStateError, CStateHealth, ForceHeartbeat, GetCollectorState, GetHealth,
        UpdateHealthMetrics, WrappedCStateMessage,
    },
    network_messages::{CSTATE_ROOM, CStateMessage},
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
use zznet_session::{
    room_message_trait::RoomMessageTrait, session_manager_like::SessionManagerLike, types::RoomId,
};

/// The main actor for the `zzcollector-state` component.
///
/// This actor manages the state of a collector instance, including its identity,
/// health, and registration with a database. It can be configured to run in
/// one of three roles: `Collector`, `Database`, or `Admin`.
pub struct CStateActor<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole + 'static,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
{
    role: CStateRole,
    collector_state: Option<CollectorStateData>,
    database_state: Option<DatabaseStateData>,
    session_manager: Option<Arc<SM>>,
    // health counters
    heartbeats_sent: Arc<AtomicU64>,
    heartbeats_acked: Arc<AtomicU64>,
    heartbeats_failed: Arc<AtomicU64>,
    _phantom: PhantomData<(TMsg, TRole)>,
}

impl<TMsg, TRole, SM> CStateActor<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole + 'static,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
{
    /// Creates a new `CStateActor`.
    pub fn new(role: CStateRole, session_manager: Option<Arc<SM>>) -> Self {
        let mut actor = Self {
            role,
            collector_state: None,
            database_state: None,
            session_manager,
            heartbeats_sent: Arc::new(AtomicU64::new(0)),
            heartbeats_acked: Arc::new(AtomicU64::new(0)),
            heartbeats_failed: Arc::new(AtomicU64::new(0)),
            _phantom: PhantomData,
        };

        match &actor.role {
            CStateRole::Collector { collector_id, .. } => {
                actor.collector_state = Some(CollectorStateData::new(collector_id.clone()));
            }
            CStateRole::Database { .. } => {
                actor.database_state = Some(DatabaseStateData::default());
            }
            CStateRole::Admin => {}
        }

        actor
    }
}

impl<TMsg, TRole, SM> CStateActor<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole + 'static,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
{
    fn start_heartbeat(&self, ctx: &mut Context<Self>) {
        if let CStateRole::Collector {
            heartbeat_interval_secs,
            ..
        } = self.role
        {
            let interval = Duration::from_secs(heartbeat_interval_secs);
            ctx.add_stream(IntervalStream::new(tokio::time::interval(interval)));
        }
    }

    fn send_heartbeat(&mut self, ctx: &mut Context<Self>) -> Result<(), CStateError> {
        let sm = self
            .session_manager
            .as_ref()
            .ok_or(CStateError::SessionManagerMissing)?;
        let state = self
            .collector_state
            .as_mut()
            .ok_or(CStateError::InvalidRole("Collector".to_string()))?;

        let msg = CStateMessage::Heartbeat {
            collector_id: state.collector_id.clone(),
            uptime_secs: state.start_time.elapsed().as_secs(),
            pings_sent: state.pings_sent,
            pings_received: state.pings_received,
            batches_sent: state.batches_sent,
            last_config_update_ms: state.last_config_update_ms,
            connection_nonce: state.connection_nonce,
        };

        let sm_clone = sm.clone();
        let room_id = RoomId::from(CSTATE_ROOM);
        let addr = ctx.address();

        // Spawn and inspect results so we can increment failure counters.
        let hb_sent_counter = self.heartbeats_sent.clone();
        let hb_failed = self.heartbeats_failed.clone();

        tokio::spawn(async move {
            let results = sm_clone
                .broadcast_to_room(&room_id, msg.into(), |_role| true, None)
                .await;

            let failures = results.iter().filter(|(_, r)| r.is_err()).count() as u64;

            hb_sent_counter.fetch_add(1, Ordering::Relaxed);
            if failures > 0 {
                hb_failed.fetch_add(failures, Ordering::Relaxed);
            }

            // If desired, notify actor of ack results via message in future
            addr.do_send(ForceHeartbeat); // noop-like to keep actor alive
        });

        state.last_heartbeat_sent_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(())
    }
}

impl<TMsg, TRole, SM> Actor for CStateActor<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole + 'static,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("CStateActor started in role: {:?}", self.role);
        self.start_heartbeat(ctx);

        // If running as Database, schedule stale cleanup
        if let CStateRole::Database {
            stale_timeout_secs, ..
        } = &self.role
        {
            // check interval = half of stale timeout, minimum 1s
            let check = std::cmp::max(1, *stale_timeout_secs / 2);
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

impl<TMsg, TRole, SM> StreamHandler<tokio::time::Instant> for CStateActor<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole + 'static,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
{
    fn handle(&mut self, _item: tokio::time::Instant, ctx: &mut Context<Self>) {
        if let Err(e) = self.send_heartbeat(ctx) {
            warn!("Failed to send heartbeat: {}", e);
            self.heartbeats_failed.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl<TMsg, TRole, SM> Handler<ForceHeartbeat> for CStateActor<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole + 'static,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
{
    type Result = Result<(), CStateError>;

    fn handle(&mut self, _msg: ForceHeartbeat, ctx: &mut Context<Self>) -> Self::Result {
        self.send_heartbeat(ctx)
    }
}

impl<TMsg, TRole, SM> Handler<WrappedCStateMessage> for CStateActor<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole + 'static,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
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
                            if let CStateRole::Database {
                                max_collectors: Some(max),
                                ..
                            } = &self.role
                            {
                                // If collector not already known and we're at capacity, reject
                                if !state.collectors.contains_key(&collector_id)
                                    && state.collectors.len() >= *max
                                {
                                    reject = true;
                                }
                            }

                            if reject {
                                warn!(
                                    "Rejecting collector registration due to max_collectors limit: {}",
                                    collector_id
                                );
                                // TODO: Consider eviction policy here (LRU/oldest eviction) instead of rejecting.
                                // Current behavior: reject new registrations when at capacity.
                                // Future work: choose and implement eviction strategy and tests.
                                if let Some(sm) = &self.session_manager {
                                    let sm_clone = sm.clone();
                                    let peer_id = msg.peer_id.clone();
                                    let room_id = RoomId::from(CSTATE_ROOM);
                                    let rej = CStateMessage::RegistrationRejected {
                                        reason: "max_collectors reached".to_string(),
                                    };
                                    tokio::spawn(async move {
                                        let _ = sm_clone
                                            .send_to_room(&peer_id, &room_id, rej.into())
                                            .await;
                                    });
                                }
                                return;
                            }

                            let collector = TrackedCollector {
                                id: collector_id.clone(),
                                last_seen_ms: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis()
                                    as u64,
                                uptime_secs,
                                pings_sent,
                                pings_received,
                                batches_sent,
                                connection_nonce,
                                peer_id: msg.peer_id.to_string(),
                            };
                            state.collectors.insert(collector_id.clone(), collector);

                            // Send HeartbeatAck back to sender
                            if let Some(sm) = &self.session_manager {
                                let ack = CStateMessage::HeartbeatAck {
                                    timestamp_ms: last_config_update_ms,
                                    server_time_ms: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis()
                                        as u64,
                                };
                                let sm_clone = sm.clone();
                                let peer_id = msg.peer_id.clone();
                                let room_id = RoomId::from(CSTATE_ROOM);
                                tokio::spawn(async move {
                                    let _ =
                                        sm_clone.send_to_room(&peer_id, &room_id, ack.into()).await;
                                });
                            }
                        }
                        CStateMessage::QueryCollectors => {
                            // Admin request: only respond if the requester is an admin.
                            if let Some(state) = &self.database_state {
                                let authorized = if let Some(sm) = &self.session_manager {
                                    // Query role information for the requesting peer
                                    sm.get_peer_role(&msg.peer_id)
                                        .map(|r| r.as_str() == "admin")
                                        .unwrap_or(false)
                                } else {
                                    // If no SessionManager configured, in debug allow
                                    #[cfg(debug_assertions)]
                                    {
                                        true
                                    }

                                    #[cfg(not(debug_assertions))]
                                    {
                                        false
                                    }
                                };

                                if !authorized {
                                    warn!(
                                        "Unauthorized QueryCollectors request from {}",
                                        msg.peer_id
                                    );
                                    // Send explicit Unauthorized response when possible
                                    if let Some(sm) = &self.session_manager {
                                        let sm_clone = sm.clone();
                                        let peer_id = msg.peer_id.clone();
                                        let room_id = RoomId::from(CSTATE_ROOM);
                                        let resp = CStateMessage::Unauthorized {
                                            reason: "insufficient privileges".to_string(),
                                        };
                                        tokio::spawn(async move {
                                            let _ = sm_clone
                                                .send_to_room(&peer_id, &room_id, resp.into())
                                                .await;
                                        });
                                    }

                                    return;
                                }

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
                                if let Some(sm) = &self.session_manager {
                                    let sm_clone = sm.clone();
                                    let peer_id = msg.peer_id.clone();
                                    let room_id = RoomId::from(CSTATE_ROOM);
                                    tokio::spawn(async move {
                                        let _ = sm_clone
                                            .send_to_room(&peer_id, &room_id, response.into())
                                            .await;
                                    });
                                }
                            }
                        }
                        other => {
                            warn!(
                                "Received unexpected message type in Database role: {:?}",
                                other
                            );
                        }
                    }
                }
            }
            CStateRole::Collector { .. } => {
                // Collector should handle HeartbeatAck from Database
                if let CStateMessage::HeartbeatAck {
                    timestamp_ms: _ts,
                    server_time_ms,
                } = msg.message
                    && let Some(state) = &mut self.collector_state
                {
                    state.last_heartbeat_ack_ms = server_time_ms;
                    self.heartbeats_acked.fetch_add(1, Ordering::Relaxed);
                }
            }
            CStateRole::Admin => {
                // Admin primarily sends requests; receiving CollectorList handled here if desired
            }
        }
    }
}

// Handle stale cleanup command
impl<TMsg, TRole, SM> Handler<crate::messages::CleanupStaleCollectors>
    for CStateActor<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole + 'static,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
{
    type Result = ();

    fn handle(
        &mut self,
        _msg: crate::messages::CleanupStaleCollectors,
        _ctx: &mut Context<Self>,
    ) -> Self::Result {
        if let CStateRole::Database {
            stale_timeout_secs, ..
        } = &self.role
            && let Some(state) = &mut self.database_state
        {
            // If caller configured 0 seconds, treat as immediate removal of all collectors
            if *stale_timeout_secs == 0 {
                state.collectors.clear();
            } else {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let timeout_ms = stale_timeout_secs.saturating_mul(1000);
                state.collectors.retain(|id, c| {
                    let age = now.saturating_sub(c.last_seen_ms);
                    if age > timeout_ms {
                        debug!("Removing stale collector: {} (age_ms={})", id, age);
                        false
                    } else {
                        true
                    }
                });
            }
        }
    }
}

impl<TMsg, TRole, SM> Handler<UpdateHealthMetrics> for CStateActor<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole + 'static,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
{
    type Result = ();

    fn handle(&mut self, msg: UpdateHealthMetrics, _ctx: &mut Context<Self>) {
        if let Some(state) = &mut self.collector_state {
            if let Some(val) = msg.pings_sent {
                state.pings_sent = val;
            }
            if let Some(val) = msg.pings_received {
                state.pings_received = val;
            }
            if let Some(val) = msg.batches_sent {
                state.batches_sent = val;
            }
            if let Some(val) = msg.last_config_update_ms {
                state.last_config_update_ms = val;
            }
        }
    }
}

impl<TMsg, TRole, SM> Handler<GetHealth> for CStateActor<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole + 'static,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
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

impl<TMsg, TRole, SM> Handler<GetCollectorState> for CStateActor<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole + 'static,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
{
    type Result = Result<CollectorStateData, CStateError>;

    fn handle(&mut self, _msg: GetCollectorState, _ctx: &mut Context<Self>) -> Self::Result {
        self.collector_state
            .clone()
            .ok_or_else(|| CStateError::InvalidRole("Collector".to_string()))
    }
}
