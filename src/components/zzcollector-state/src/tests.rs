use crate::{
    builder::CStateBuilder,
    messages::{GetCollectorState, UpdateHealthMetrics, WrappedCStateMessage},
    network_messages::CStateMessage,
    role::CStateRole,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

// Local MockRole for tests (avoids requiring zznet_auth test-utils feature)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MockRole {
    Admin,
    Database,
    Collector,
}

impl zznet_auth::role::ApplicationRole for MockRole {
    fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
        match cn {
            "admin" => Ok(MockRole::Admin),
            "database" => Ok(MockRole::Database),
            "collector" => Ok(MockRole::Collector),
            _ => Err(zznet_auth::error::AuthError::UnknownRole(cn.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            MockRole::Admin => "admin",
            MockRole::Database => "database",
            MockRole::Collector => "collector",
        }
    }

    fn can_connect_to(&self, _target: &Self) -> bool {
        true
    }

    fn can_access_room(&self, _room_name: &str) -> bool {
        true
    }
}
use zznet_session::{
    session_manager::SessionManager,
    session_manager_like::SessionManagerLike,
    types::{PeerId, RoomId, SessionError},
};

// Mock SessionManager
#[derive(Default)]
pub struct MockSessionManager {
    broadcast_log: Arc<Mutex<Vec<CStateMessage>>>,
    send_log: Arc<Mutex<Vec<CStateMessage>>>,
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
        self.send_log.lock().unwrap().push(_msg);
        Ok(())
    }

    fn get_peer_role(&self, peer_id: &PeerId) -> Option<MockRole> {
        // Simple mapping for test scenarios: any peer containing "admin" is Admin,
        // peers containing "test" are Database, others are Collector.
        let s = peer_id.as_str();
        if s.contains("admin") {
            Some(MockRole::Admin)
        } else if s.contains("test") {
            Some(MockRole::Database)
        } else {
            Some(MockRole::Collector)
        }
    }
}

#[actix::test]
async fn test_collector_role_heartbeat() {
    let sm = Arc::new(MockSessionManager::default());
    let broadcast_log = sm.broadcast_log.clone();
    let send_log = sm.send_log.clone();

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
    assert!(!log.is_empty(), "expected at least one heartbeat");
    assert!(matches!(log[0], CStateMessage::Heartbeat { .. }));
    // ensure no direct send_to_room occurred for heartbeat broadcast
    let slog = send_log.lock().unwrap();
    assert!(slog.is_empty());
}

#[actix::test]
async fn test_database_role_sends_ack_and_query_response() {
    let sm = Arc::new(MockSessionManager::default());
    let send_log = sm.send_log.clone();

    let role = CStateRole::Database {
        stale_timeout_secs: 10,
        max_collectors: None,
    };

    let builder =
        CStateBuilder::<CStateMessage, MockRole, MockSessionManager>::new(role).session_manager(sm);
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

    // Ensure an ack was sent
    {
        let sent = send_log.lock().unwrap();
        assert!(
            sent.iter()
                .any(|m| matches!(m, CStateMessage::HeartbeatAck { .. }))
        );
    }

    // Now send QueryCollectors from admin peer and ensure CollectorList response
    let query = WrappedCStateMessage {
        peer_id: PeerId::from("admin-peer"),
        message: CStateMessage::QueryCollectors,
    };

    actor.send(query).await.unwrap();

    {
        let sent = send_log.lock().unwrap();
        assert!(
            sent.iter()
                .any(|m| matches!(m, CStateMessage::CollectorList { .. }))
        );
    }
}

#[actix::test]
async fn test_unauthorized_query_collectors_is_denied() {
    let sm = Arc::new(MockSessionManager::default());
    let send_log = sm.send_log.clone();

    let role = CStateRole::Database {
        stale_timeout_secs: 10,
        max_collectors: None,
    };

    let builder =
        CStateBuilder::<CStateMessage, MockRole, MockSessionManager>::new(role).session_manager(sm);
    let actor = builder.build();

    // Query from a non-admin peer (peer id without 'admin' in it per MockSessionManager)
    let query = WrappedCStateMessage {
        peer_id: PeerId::from("some-peer"),
        message: CStateMessage::QueryCollectors,
    };

    actor.send(query).await.unwrap();

    // Ensure no CollectorList was sent
    let sent = send_log.lock().unwrap();
    assert!(
        !sent
            .iter()
            .any(|m| matches!(m, CStateMessage::CollectorList { .. })),
        "expected no CollectorList for unauthorized peer"
    );
}

#[actix::test]
async fn test_max_collectors_rejection() {
    let sm = Arc::new(MockSessionManager::default());
    let send_log = sm.send_log.clone();

    let role = CStateRole::Database {
        stale_timeout_secs: 10,
        max_collectors: Some(1),
    };

    let builder =
        CStateBuilder::<CStateMessage, MockRole, MockSessionManager>::new(role).session_manager(sm);
    let actor = builder.build();

    // First heartbeat - should be accepted
    let msg1 = WrappedCStateMessage {
        peer_id: PeerId::from("peer-1"),
        message: CStateMessage::Heartbeat {
            collector_id: "collector-1".to_string(),
            uptime_secs: 10,
            pings_sent: 1,
            pings_received: 1,
            batches_sent: 1,
            last_config_update_ms: 1,
            connection_nonce: 1,
        },
    };

    actor.send(msg1).await.unwrap();

    // Second heartbeat from a different collector should be rejected due to max_collectors=1
    let msg2 = WrappedCStateMessage {
        peer_id: PeerId::from("peer-2"),
        message: CStateMessage::Heartbeat {
            collector_id: "collector-2".to_string(),
            uptime_secs: 5,
            pings_sent: 0,
            pings_received: 0,
            batches_sent: 0,
            last_config_update_ms: 1,
            connection_nonce: 2,
        },
    };

    actor.send(msg2).await.unwrap();

    // Check that a RegistrationRejected was sent to peer-2
    let sent = send_log.lock().unwrap();
    assert!(
        sent.iter()
            .any(|m| matches!(m, CStateMessage::RegistrationRejected { .. })),
        "expected RegistrationRejected for overflow collector"
    );
}

#[actix::test]
async fn test_stale_collector_cleanup() {
    let sm = Arc::new(MockSessionManager::default());
    let send_log = sm.send_log.clone();

    // Use 0s stale timeout to force immediate cleanup
    let role = CStateRole::Database {
        stale_timeout_secs: 0,
        max_collectors: None,
    };

    let builder =
        CStateBuilder::<CStateMessage, MockRole, MockSessionManager>::new(role).session_manager(sm);
    let actor = builder.build();

    // Simulate heartbeat
    let msg = WrappedCStateMessage {
        peer_id: PeerId::from("test-peer"),
        message: CStateMessage::Heartbeat {
            collector_id: "stale-collector".to_string(),
            uptime_secs: 10,
            pings_sent: 1,
            pings_received: 1,
            batches_sent: 1,
            last_config_update_ms: 1,
            connection_nonce: 1,
        },
    };

    actor.send(msg).await.unwrap();

    // Trigger cleanup immediately
    actor
        .send(crate::messages::CleanupStaleCollectors)
        .await
        .unwrap();

    // Query collectors and check CollectorList response
    let query = WrappedCStateMessage {
        peer_id: PeerId::from("admin-peer"),
        message: CStateMessage::QueryCollectors,
    };
    actor.send(query).await.unwrap();

    let sent = send_log.lock().unwrap();
    // There should be a CollectorList response; since timeout was 0, it should be empty
    let found = sent.iter().find_map(|m| {
        if let CStateMessage::CollectorList { collectors } = m {
            Some(collectors.len())
        } else {
            None
        }
    });
    assert!(matches!(found, Some(0)));
}

#[actix::test]
async fn test_database_role_receives_heartbeat() {
    let role = CStateRole::Database {
        stale_timeout_secs: 10,
        max_collectors: None,
    };

    let builder =
        CStateBuilder::<CStateMessage, MockRole, SessionManager<CStateMessage, MockRole>>::new(
            role,
        );
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

    let builder =
        CStateBuilder::<CStateMessage, MockRole, SessionManager<CStateMessage, MockRole>>::new(
            role,
        );
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

#[actix::test]
async fn test_unauthorized_response_sent() {
    let sm = Arc::new(MockSessionManager::default());
    let send_log = sm.send_log.clone();

    let role = CStateRole::Database {
        stale_timeout_secs: 10,
        max_collectors: None,
    };

    let builder =
        CStateBuilder::<CStateMessage, MockRole, MockSessionManager>::new(role).session_manager(sm);
    let actor = builder.build();

    let query = WrappedCStateMessage {
        peer_id: PeerId::from("some-peer"),
        message: CStateMessage::QueryCollectors,
    };

    actor.send(query).await.unwrap();

    let sent = send_log.lock().unwrap();
    assert!(
        sent.iter()
            .any(|m| matches!(m, CStateMessage::Unauthorized { .. })),
        "expected Unauthorized message sent back"
    );
}

#[actix::test]
async fn test_collector_receives_ack_increments_counter() {
    let sm = Arc::new(MockSessionManager::default());

    let role = CStateRole::Collector {
        collector_id: "test-collector".to_string(),
        heartbeat_interval_secs: 1,
    };

    let builder =
        CStateBuilder::<CStateMessage, MockRole, MockSessionManager>::new(role).session_manager(sm);
    let actor = builder.build();

    // Simulate database ack being sent to collector
    let ack = WrappedCStateMessage {
        peer_id: PeerId::from("db-peer"),
        message: CStateMessage::HeartbeatAck {
            timestamp_ms: 1,
            server_time_ms: 42,
        },
    };

    actor.send(ack).await.unwrap();

    // After ack, the collector's health should show 1 ack
    let health = actor
        .send(crate::messages::GetHealth)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(health.heartbeats_acked, 1);
}

#[actix::test]
async fn test_stale_collector_cleanup_deterministic() {
    // Force immediate cleanup by using 0s stale timeout
    let sm = Arc::new(MockSessionManager::default());
    let send_log = sm.send_log.clone();

    let role = CStateRole::Database {
        stale_timeout_secs: 0,
        max_collectors: None,
    };

    let builder =
        CStateBuilder::<CStateMessage, MockRole, MockSessionManager>::new(role).session_manager(sm);
    let actor = builder.build();

    // Simulate heartbeat
    let now_msg = WrappedCStateMessage {
        peer_id: PeerId::from("test-peer"),
        message: CStateMessage::Heartbeat {
            collector_id: "stale-collector".to_string(),
            uptime_secs: 10,
            pings_sent: 1,
            pings_received: 1,
            batches_sent: 1,
            last_config_update_ms: 1,
            connection_nonce: 1,
        },
    };

    actor.send(now_msg).await.unwrap();

    // Trigger cleanup immediately (0s configured)
    actor
        .send(crate::messages::CleanupStaleCollectors)
        .await
        .unwrap();

    // Query collectors as admin and expect empty list
    let query = WrappedCStateMessage {
        peer_id: PeerId::from("admin-peer"),
        message: CStateMessage::QueryCollectors,
    };
    actor.send(query).await.unwrap();

    let sent = send_log.lock().unwrap();
    let found = sent.iter().find_map(|m| {
        if let CStateMessage::CollectorList { collectors } = m {
            Some(collectors.len())
        } else {
            None
        }
    });
    assert!(matches!(found, Some(0)));
}

#[actix::test]
async fn test_multiple_collectors_tracked() {
    let sm = Arc::new(MockSessionManager::default());
    let send_log = sm.send_log.clone();

    let role = CStateRole::Database {
        stale_timeout_secs: 60,
        max_collectors: None,
    };

    let builder =
        CStateBuilder::<CStateMessage, MockRole, MockSessionManager>::new(role).session_manager(sm);
    let actor = builder.build();

    let mk_msg = |peer: &str, id: &str| WrappedCStateMessage {
        peer_id: PeerId::from(peer),
        message: CStateMessage::Heartbeat {
            collector_id: id.to_string(),
            uptime_secs: 10,
            pings_sent: 1,
            pings_received: 1,
            batches_sent: 1,
            last_config_update_ms: 1,
            connection_nonce: 1,
        },
    };

    actor.send(mk_msg("peer-a", "collector-a")).await.unwrap();
    actor.send(mk_msg("peer-b", "collector-b")).await.unwrap();

    // Query collectors as admin
    let query = WrappedCStateMessage {
        peer_id: PeerId::from("admin-peer"),
        message: CStateMessage::QueryCollectors,
    };
    actor.send(query).await.unwrap();

    let sent = send_log.lock().unwrap();
    let found = sent.iter().find_map(|m| {
        if let CStateMessage::CollectorList { collectors } = m {
            Some(collectors.len())
        } else {
            None
        }
    });
    assert!(matches!(found, Some(n) if n >= 2));
}
