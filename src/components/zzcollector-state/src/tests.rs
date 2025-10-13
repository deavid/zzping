use crate::{
    builder::CStateBuilder,
    messages::{GetCollectorState, UpdateHealthMetrics, WrappedCStateMessage},
    network_messages::CStateMessage,
    role::CStateRole,
};
use async_trait::async_trait;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use zznet_auth::mock::MockRole;
use zznet_session::{
    room_message_trait::{
        DeserializationError, RoomMessageTrait, SerializationError,
    },
    session_manager::SessionManager,
    session_manager_like::SessionManagerLike,
    types::{PeerId, RoomId, SessionError},
};

// A mock message type for testing
#[derive(Clone, Debug, PartialEq)]
pub enum MockMessage {}

impl RoomMessageTrait for MockMessage {
    fn room_id(&self) -> RoomId {
        unimplemented!()
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        unimplemented!()
    }

    fn deserialize_for_room(
        _room_id: &RoomId,
        _bytes: &[u8],
    ) -> Result<Self, DeserializationError> {
        unimplemented!()
    }

    fn supported_rooms() -> Vec<RoomId> {
        unimplemented!()
    }
}

impl From<CStateMessage> for MockMessage {
    fn from(_msg: CStateMessage) -> Self {
        unimplemented!()
    }
}

// Mock SessionManager
#[derive(Default)]
pub struct MockSessionManager {
    broadcast_log: Arc<Mutex<Vec<CStateMessage>>>,
}

#[async_trait]
impl SessionManagerLike<CStateMessage, MockRole> for MockSessionManager {
    async fn broadcast_to_room<F>(
        &self,
        _room_id: &RoomId,
        message: CStateMessage,
        _filter: F,
        _timeout: Option<Duration>,
    ) -> Vec<(PeerId, Result<(), SessionError>)>
    where
        F: Fn(&MockRole) -> bool + Send + Sync + 'static,
    {
        self.broadcast_log.lock().unwrap().push(message);
        vec![]
    }

    async fn send_to_room(
        &self,
        _peer_id: &PeerId,
        _room_id: &RoomId,
        _msg: CStateMessage,
    ) -> Result<(), SessionError> {
        Ok(())
    }
}

#[actix::test]
async fn test_collector_role_heartbeat() {
    let sm = Arc::new(MockSessionManager::default());
    let broadcast_log = sm.broadcast_log.clone();

    let role = CStateRole::Collector {
        collector_id: "test-collector".to_string(),
        heartbeat_interval_secs: 1,
    };

    let builder =
        CStateBuilder::<CStateMessage, MockRole, MockSessionManager>::new(role).session_manager(sm);
    let _actor = builder.build();

    // Wait for a heartbeat to be sent
    tokio::time::sleep(Duration::from_secs(2)).await;
    actix::System::current().stop();

    let log = broadcast_log.lock().unwrap();
    assert_eq!(log.len(), 2);
    assert!(matches!(log[0], CStateMessage::Heartbeat { .. }));
}

#[actix::test]
async fn test_database_role_receives_heartbeat() {
    let role = CStateRole::Database {
        stale_timeout_secs: 10,
        max_collectors: None,
    };

    let builder = CStateBuilder::<CStateMessage, MockRole, SessionManager<CStateMessage, MockRole>>::new(role);
    let actor = builder.build();

    let msg = WrappedCStateMessage {
        peer_id: PeerId::from("test-peer"),
        message: CStateMessage::Heartbeat {
            collector_id: "test-collector".to_string(),
            uptime_secs: 10,
            pings_sent: 1,
            pings_received: 1,
            batches_sent: 1,
            last_config_update_ms: 1,
            connection_nonce: 1,
        },
    };

    actor.send(msg).await.unwrap();
}

#[actix::test]
async fn test_update_health_metrics() {
    let role = CStateRole::Collector {
        collector_id: "test-collector".to_string(),
        heartbeat_interval_secs: 999,
    };

    let builder = CStateBuilder::<CStateMessage, MockRole, SessionManager<CStateMessage, MockRole>>::new(role);
    let actor = builder.build();

    let metrics = UpdateHealthMetrics {
        pings_sent: Some(123),
        pings_received: Some(123),
        batches_sent: Some(123),
        last_config_update_ms: Some(123),
    };

    actor.send(metrics).await.unwrap();

    let state = actor.send(GetCollectorState).await.unwrap().unwrap();
    assert_eq!(state.pings_sent, 123);
    assert_eq!(state.pings_received, 123);
    assert_eq!(state.batches_sent, 123);
    assert_eq!(state.last_config_update_ms, 123);
}