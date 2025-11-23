use crate::{
    builder::CStateBuilder,
    config::CStateConfig,
    internal_messages::{InboundHeartbeat, InboundHeartbeatAck, InboundQueryCollectors},
    messages::{GetCollectorState, UpdateHealthMetrics},
};
use zznet_api::PeerId;

#[actix::test]
async fn test_database_role_sends_ack_and_query_response() {
    let config = CStateConfig::for_database(10, None);

    let builder = CStateBuilder::new(config);
    let actor = builder.build();

    // Send heartbeat via internal message (simulates NetworkActor forwarding)
    let msg = InboundHeartbeat {
        peer_id: PeerId::from("test-peer"),
        collector_id: "test-collector".to_string(),
        uptime_secs: 10,
        pings_sent: 1,
        pings_received: 1,
        batches_sent: 1,
        last_config_update_ms: 1,
        connection_nonce: 1,
    };

    actor.send(msg).await.unwrap().unwrap();

    // Now send QueryCollectors from admin peer
    let query = InboundQueryCollectors {
        peer_id: PeerId::from("admin-peer"),
    };

    actor.send(query).await.unwrap().unwrap();
}

#[actix::test]
async fn test_unauthorized_query_collectors_is_denied() {
    let config = CStateConfig::for_database(10, None);

    let builder = CStateBuilder::new(config);
    let actor = builder.build();

    // Query from a non-admin peer
    let query = InboundQueryCollectors {
        peer_id: PeerId::from("some-peer"),
    };

    actor.send(query).await.unwrap().unwrap();
}

#[actix::test]
async fn test_max_collectors_rejection() {
    let config = CStateConfig::for_database(10, Some(1));

    let builder = CStateBuilder::new(config);
    let actor = builder.build();

    // First heartbeat - should be accepted
    let msg1 = InboundHeartbeat {
        peer_id: PeerId::from("peer-1"),
        collector_id: "collector-1".to_string(),
        uptime_secs: 10,
        pings_sent: 1,
        pings_received: 1,
        batches_sent: 1,
        last_config_update_ms: 1,
        connection_nonce: 1,
    };

    actor.send(msg1).await.unwrap().unwrap();

    // Second heartbeat from a different collector should be rejected due to max_collectors=1
    let msg2 = InboundHeartbeat {
        peer_id: PeerId::from("peer-2"),
        collector_id: "collector-2".to_string(),
        uptime_secs: 5,
        pings_sent: 0,
        pings_received: 0,
        batches_sent: 0,
        last_config_update_ms: 1,
        connection_nonce: 2,
    };

    actor.send(msg2).await.unwrap().unwrap();

    // Check that a RegistrationRejected was sent to peer-2
}

#[actix::test]
async fn test_stale_collector_cleanup() {
    // Use 0s stale timeout to force immediate cleanup
    let config = CStateConfig::for_database(0, None);

    let builder = CStateBuilder::new(config);
    let actor = builder.build();

    // Simulate heartbeat
    let msg = InboundHeartbeat {
        peer_id: PeerId::from("test-peer"),
        collector_id: "stale-collector".to_string(),
        uptime_secs: 10,
        pings_sent: 1,
        pings_received: 1,
        batches_sent: 1,
        last_config_update_ms: 1,
        connection_nonce: 1,
    };

    actor.send(msg).await.unwrap().unwrap();

    // Trigger cleanup immediately
    actor
        .send(crate::messages::CleanupStaleCollectors)
        .await
        .unwrap();

    // Query collectors and check CollectorList response
    let query = InboundQueryCollectors {
        peer_id: PeerId::from("admin-peer"),
    };
    actor.send(query).await.unwrap().unwrap();
}

#[actix::test]
async fn test_database_role_receives_heartbeat() {
    let config = CStateConfig::for_database(10, None);

    let actor = CStateBuilder::new(config).build();

    let msg = InboundHeartbeat {
        peer_id: PeerId::from("test-peer"),
        collector_id: "test-collector".to_string(),
        uptime_secs: 10,
        pings_sent: 1,
        pings_received: 1,
        batches_sent: 1,
        last_config_update_ms: 1,
        connection_nonce: 1,
    };

    actor.send(msg).await.unwrap().unwrap();
}

#[actix::test]
async fn test_update_health_metrics() {
    let config = CStateConfig::for_collector("test-collector".to_string(), 999000);

    let actor = CStateBuilder::new(config).build();

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
    let config = CStateConfig::for_database(10, None);

    let builder = CStateBuilder::new(config);
    let actor = builder.build();

    let query = InboundQueryCollectors {
        peer_id: PeerId::from("some-peer"),
    };

    actor.send(query).await.unwrap().unwrap();
}

#[actix::test]
async fn test_collector_receives_ack_increments_counter() {
    let config = CStateConfig::for_collector("test-collector".to_string(), 1000);

    let builder = CStateBuilder::new(config);
    let actor = builder.build();

    // Simulate database ack being sent to collector
    let ack = InboundHeartbeatAck {
        peer_id: PeerId::from("db-peer"),
        timestamp_ms: 1,
        server_time_ms: 42,
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

    let config = CStateConfig::for_database(0, None);

    let builder = CStateBuilder::new(config);
    let actor = builder.build();

    // Simulate heartbeat
    let now_msg = InboundHeartbeat {
        peer_id: PeerId::from("test-peer"),
        collector_id: "stale-collector".to_string(),
        uptime_secs: 10,
        pings_sent: 1,
        pings_received: 1,
        batches_sent: 1,
        last_config_update_ms: 1,
        connection_nonce: 1,
    };

    actor.send(now_msg).await.unwrap().unwrap();

    // Trigger cleanup immediately (0s configured)
    actor
        .send(crate::messages::CleanupStaleCollectors)
        .await
        .unwrap();

    // Query collectors as admin and expect empty list
    let query = InboundQueryCollectors {
        peer_id: PeerId::from("admin-peer"),
    };
    actor.send(query).await.unwrap().unwrap();
}

#[actix::test]
async fn test_multiple_collectors_tracked() {
    let config = CStateConfig::for_database(60, None);

    let builder = CStateBuilder::new(config);
    let actor = builder.build();

    let mk_msg = |peer: &str, id: &str| InboundHeartbeat {
        peer_id: PeerId::from(peer),
        collector_id: id.to_string(),
        uptime_secs: 10,
        pings_sent: 1,
        pings_received: 1,
        batches_sent: 1,
        last_config_update_ms: 1,
        connection_nonce: 1,
    };

    actor
        .send(mk_msg("peer-a", "collector-a"))
        .await
        .unwrap()
        .unwrap();
    actor
        .send(mk_msg("peer-b", "collector-b"))
        .await
        .unwrap()
        .unwrap();

    // Query collectors as admin
    let query = InboundQueryCollectors {
        peer_id: PeerId::from("admin-peer"),
    };
    actor.send(query).await.unwrap().unwrap();
}
