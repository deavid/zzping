//! The core actor implementation for the `zzcollector-state` component.

use crate::{
    messages::{
        CStateError, CStateHealth, ForceHeartbeat, GetCollectorState, GetHealth,
        UpdateHealthMetrics, WrappedCStateMessage,
    },
    network_messages::{CStateMessage, CSTATE_ROOM},
    role::CStateRole,
    state::{CollectorStateData, DatabaseStateData, TrackedCollector},
};
use actix::prelude::*;
use log::{debug, info, warn};
use std::{marker::PhantomData, sync::Arc, time::Duration};
use tokio_stream::wrappers::IntervalStream;
use zznet_auth::ApplicationRole;
use zznet_session::{
    room_message_trait::RoomMessageTrait, session_manager_like::SessionManagerLike,
    types::RoomId,
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
    heartbeats_sent: u64,
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
            heartbeats_sent: 0,
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

    fn send_heartbeat(&mut self) -> Result<(), CStateError> {
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

        tokio::spawn(async move {
            sm_clone
                .broadcast_to_room(
                    &room_id,
                    msg.into(),
                    |_role| true, // Broadcast to all for now
                    None,
                )
                .await;
        });

        state.last_heartbeat_sent_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        self.heartbeats_sent += 1;

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
    }
}

impl<TMsg, TRole, SM> StreamHandler<tokio::time::Instant> for CStateActor<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole + 'static,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
{
    fn handle(&mut self, _item: tokio::time::Instant, _ctx: &mut Context<Self>) {
        if let Err(e) = self.send_heartbeat() {
            warn!("Failed to send heartbeat: {}", e);
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

    fn handle(&mut self, _msg: ForceHeartbeat, _ctx: &mut Context<Self>) -> Self::Result {
        self.send_heartbeat()
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
        if let CStateRole::Database { .. } = &self.role {
            if let Some(state) = &mut self.database_state {
                match msg.message {
                    CStateMessage::Heartbeat {
                        collector_id,
                        uptime_secs,
                        pings_sent,
                        pings_received,
                        batches_sent,
                        connection_nonce,
                        ..
                    } => {
                        debug!("Received heartbeat from collector: {}", collector_id);
                        let collector = TrackedCollector {
                            id: collector_id.clone(),
                            last_seen_ms: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as u64,
                            uptime_secs,
                            pings_sent,
                            pings_received,
                            batches_sent,
                            connection_nonce,
                            peer_id: msg.peer_id.to_string(),
                        };
                        state.collectors.insert(collector_id, collector);
                    }
                    _ => {
                        warn!("Received unexpected message type in Database role");
                    }
                }
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
            heartbeats_sent: self.heartbeats_sent,
            heartbeats_acked: 0, // Not implemented yet
            heartbeats_failed: 0, // Not implemented yet
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