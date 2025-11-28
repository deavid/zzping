//! Tests for the zzcollector-state component
//!
//! These tests verify the "Control Plane" logic for collector registration
//! and health monitoring. This "Sociable Unit Test" proves that the Database
//! correctly tracks active collectors, expires dead ones, and enforces
//! permissions and capacity limits.
//!
//! # Architecture: "Sociable Unit Test"
//!
//! Real actors talking via mocked transport. This pattern tests the full
//! three-actor system (MainActor + NetworkManager + NetworkActor per peer)
//! with actual message serialization and routing.
//!
//! # Scenarios Covered
//!
//! 1. **Roll Call** - Collector successfully registers and maintains presence
//! 2. **Ghost** - Dead collectors are removed from the registry after timeout
//! 3. **Impostor** - Only authorized roles can register as collectors
//! 4. **Capacity** - Maximum collector limit is enforced

use crate::builder::CStateBuilder;
use crate::config::CStateConfig;
use crate::messages::CleanupStaleCollectors;
use crate::network_messages::{CSTATE_ROOM, CStateMessage};
use crate::permissions::CStatePermissions;
use actix::prelude::*;
use std::collections::HashMap;
use zznet_api::{
    Frame, OnPeerConnected, PeerId, Role, RoomFrame, RoomId, TransportFrame,
    create_mock_pair,
};
use zznet_router::RouterActor;

// ============================================================================
// Helper Functions
// ============================================================================

/// Create permissions for a collector role (can send heartbeats)
fn collector_permissions() -> CStatePermissions {
    CStatePermissions::for_collector()
}

/// Create permissions for an admin role (can query collectors)
fn admin_permissions() -> CStatePermissions {
    CStatePermissions::for_admin()
}

/// Create permissions that deny all actions (default for unauthorized roles)
fn deny_all_permissions() -> CStatePermissions {
    CStatePermissions::deny_all()
}

/// Helper to serialize a CStateMessage for sending via InboundRoomPayload
fn serialize_message(msg: &CStateMessage) -> Vec<u8> {
    use zznet_room::RoomMessageTrait;
    msg.serialize_inner().expect("Serialization should succeed")
}

/// Helper to create a TransportFrame from a CStateMessage
fn create_transport_frame(msg: &CStateMessage) -> TransportFrame {
    let payload = serialize_message(msg);

    let room_frame = RoomFrame::Message {
        from_room: CSTATE_ROOM.to_string(),
        to_room: CSTATE_ROOM.to_string(),
        payload,
    };

    let frame = Frame::Room(room_frame);
    TransportFrame::new(
        frame
            .serialize()
            .expect("Frame serialization should succeed"),
    )
}

/// Helper to unwrap and deserialize a TransportFrame back to CStateMessage
fn unwrap_message(transport_frame: TransportFrame) -> CStateMessage {
    use zznet_room::RoomMessageTrait;

    // Step 1: Deserialize TransportFrame to Frame
    let frame = Frame::deserialize(transport_frame.get_bytes())
        .expect("Frame deserialization should succeed");

    // Step 2: Extract RoomFrame
    let room_frame = match frame {
        Frame::Room(rf) => rf,
        _ => panic!("Expected Room frame"),
    };

    // Step 3: Extract payload from RoomFrame::Message
    let payload = match room_frame {
        RoomFrame::Message { payload, .. } => payload,
        _ => panic!("Expected Message room frame"),
    };

    // Step 4: Deserialize payload to CStateMessage
    CStateMessage::deserialize_for_room(&RoomId::from(CSTATE_ROOM), &payload)
        .expect("Network message deserialization should succeed")
}

/// Helper to create a Heartbeat message
fn create_heartbeat(collector_id: &str, connection_nonce: u64) -> CStateMessage {
    CStateMessage::Heartbeat {
        collector_id: collector_id.to_string(),
        uptime_secs: 0,
        pings_sent: 0,
        pings_received: 0,
        batches_sent: 0,
        last_config_update_ms: 0,
        connection_nonce,
    }
}

// ============================================================================
// Test 1: The Roll Call
// ============================================================================

/// Test the full registration flow:
/// 1. Collector connects and sends Heartbeat
/// 2. Database responds with HeartbeatAck
/// 3. Admin queries and sees the collector in the list
///
/// This test verifies:
/// - Heartbeat registration works
/// - HeartbeatAck is sent back
/// - QueryCollectors returns registered collectors
#[actix_rt::test]
async fn test_roll_call() {
    // Initialize logging for debugging
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    log::info!("=== Test: Roll Call (Happy Path) ===");

    log::info!("=== Phase 1: Setup Database Node ===");

    // 1. Start the Database Router
    let db_router = RouterActor::new(vec![]).start();
    log::debug!("Database router started");

    // 2. Create Database component (tracks collectors, allows queries)
    let db_config = CStateConfig::for_database(10_000, None); // 10 second stale timeout

    let mut db_permissions = HashMap::new();
    db_permissions.insert("collector".to_string(), collector_permissions());
    db_permissions.insert("admin".to_string(), admin_permissions());

    let _db_actor = CStateBuilder::new(db_config)
        .router(db_router.clone())
        .permissions_map(db_permissions)
        .build();

    log::debug!("Database actor started");

    // Give actors time to register with router
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    log::info!("=== Phase 2: Collector Arrives and Sends Heartbeat ===");

    // 3. Create mock transport pair for Collector <-> Database
    let (trans_coll_to_db, trans_db_to_coll) = create_mock_pair("coll_db_link");

    // 4. Connect Database side (sees collector connection)
    let db_conn = trans_db_to_coll.into_established();
    let (db_send_tx, db_recv_rx, _watcher) = (db_conn.tx, db_conn.rx, db_conn.watcher);

    let db_routing_map = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("coll-A"),
            role: Role::new("collector"),
            negotiated_rooms: vec![RoomId::from(CSTATE_ROOM)],
            transport_tx: db_send_tx,
        })
        .await
        .expect("Database router should respond")
        .expect("Database connection should succeed");

    log::debug!(
        "Database connected to collector, routing map: {:?}",
        db_routing_map.keys()
    );

    // 5. Get room recipient for routing inbound messages
    let db_room_recipient = db_routing_map
        .get(&RoomId::from(CSTATE_ROOM))
        .expect("cstate room should exist")
        .clone();

    // 6. Start collector transport (to send messages)
    let coll_conn = trans_coll_to_db.into_established();
    let (coll_send_tx, mut coll_recv_rx, _watcher) = (coll_conn.tx, coll_conn.rx, coll_conn.watcher);

    // 7. Route incoming messages from collector to database
    actix::spawn(async move {
        let mut rx = db_recv_rx;
        while let Some(result) = rx.recv().await {
            match result {
                Ok(frame) => {
                    log::debug!(
                        "Routing frame to database: {} bytes",
                        frame.get_bytes().len()
                    );

                    let inner_frame = match Frame::deserialize(frame.get_bytes()) {
                        Ok(f) => f,
                        Err(e) => {
                            log::error!("Failed to deserialize frame: {:?}", e);
                            continue;
                        }
                    };

                    let payload = match inner_frame {
                        Frame::Room(RoomFrame::Message { payload, .. }) => payload,
                        _ => {
                            log::error!("Unexpected frame type");
                            continue;
                        }
                    };

                    db_room_recipient.do_send(zznet_api::InboundRoomPayload { payload });
                }
                Err(e) => {
                    log::error!("Transport error in database receive loop: {:?}", e);
                    break;
                }
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // 8. Send Heartbeat from collector
    let heartbeat = create_heartbeat("coll-A", 123);
    let transport_frame = create_transport_frame(&heartbeat);

    coll_send_tx
        .send(transport_frame)
        .await
        .expect("Send should succeed");

    log::debug!("Collector sent Heartbeat");

    // 9. Wait for HeartbeatAck
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    let response = coll_recv_rx.recv().await.expect("Should receive response");
    let response_frame = response.expect("Response should be Ok");
    let response_msg = unwrap_message(response_frame);

    log::debug!("Received response: {:?}", response_msg);

    match response_msg {
        CStateMessage::HeartbeatAck {
            timestamp_ms,
            server_time_ms,
        } => {
            log::info!(
                "✓ Received HeartbeatAck: timestamp={}, server_time={}",
                timestamp_ms,
                server_time_ms
            );
        }
        other => panic!("Expected HeartbeatAck, got {:?}", other),
    }

    log::info!("=== Phase 3: Admin Queries Collector List ===");

    // 10. Create mock transport pair for Admin <-> Database
    let (trans_admin_to_db, trans_db_to_admin) = create_mock_pair("admin_db_link");

    // 11. Connect Admin to Database
    let db_admin_conn = trans_db_to_admin.into_established();
    let (db_admin_tx, db_admin_recv_rx, _watcher) = (db_admin_conn.tx, db_admin_conn.rx, db_admin_conn.watcher);

    let db_admin_routing_map = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("admin"),
            role: Role::new("admin"),
            negotiated_rooms: vec![RoomId::from(CSTATE_ROOM)],
            transport_tx: db_admin_tx,
        })
        .await
        .expect("Database router should respond")
        .expect("Admin connection should succeed");

    log::debug!(
        "Admin connected to database, routing map: {:?}",
        db_admin_routing_map.keys()
    );

    let db_admin_room_recipient = db_admin_routing_map
        .get(&RoomId::from(CSTATE_ROOM))
        .expect("cstate room should exist")
        .clone();

    let admin_conn = trans_admin_to_db.into_established();
    let (admin_send_tx, mut admin_recv_rx, _watcher) = (admin_conn.tx, admin_conn.rx, admin_conn.watcher);

    // Route incoming messages from admin to database
    actix::spawn(async move {
        let mut rx = db_admin_recv_rx;
        while let Some(result) = rx.recv().await {
            match result {
                Ok(frame) => {
                    let inner_frame = match Frame::deserialize(frame.get_bytes()) {
                        Ok(f) => f,
                        Err(e) => {
                            log::error!("Failed to deserialize frame: {:?}", e);
                            continue;
                        }
                    };

                    let payload = match inner_frame {
                        Frame::Room(RoomFrame::Message { payload, .. }) => payload,
                        _ => continue,
                    };

                    db_admin_room_recipient.do_send(zznet_api::InboundRoomPayload { payload });
                }
                Err(e) => {
                    log::error!("Transport error: {:?}", e);
                    break;
                }
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // 12. Send QueryCollectors from admin
    let query = CStateMessage::QueryCollectors;
    let transport_frame = create_transport_frame(&query);

    admin_send_tx
        .send(transport_frame)
        .await
        .expect("Send should succeed");

    log::debug!("Admin sent QueryCollectors");

    // 13. Wait for CollectorList
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    let response = admin_recv_rx.recv().await.expect("Should receive response");
    let response_frame = response.expect("Response should be Ok");
    let response_msg = unwrap_message(response_frame);

    log::debug!("Received response: {:?}", response_msg);

    match response_msg {
        CStateMessage::CollectorList { collectors } => {
            assert_eq!(collectors.len(), 1, "Should have 1 collector");
            assert_eq!(collectors[0].id, "coll-A", "Collector ID should match");
            log::info!(
                "✓ Received CollectorList with {} collector(s): {:?}",
                collectors.len(),
                collectors.iter().map(|c| &c.id).collect::<Vec<_>>()
            );
        }
        other => panic!("Expected CollectorList, got {:?}", other),
    }

    log::info!("=== Test Complete ===");
    log::info!(
        "Verified: Heartbeat registration, HeartbeatAck response, QueryCollectors returns registered collector"
    );
}

// ============================================================================
// Test 2: The Ghost (Stale Cleanup)
// ============================================================================

/// Test that dead collectors are removed from the registry after timeout:
/// 1. Collector registers via Heartbeat
/// 2. Wait for stale timeout to expire (1ms)
/// 3. Trigger cleanup
/// 4. Admin query returns empty list
///
/// This test verifies:
/// - Stale timeout mechanism works
/// - CleanupStaleCollectors removes expired collectors
/// - Query returns empty list after cleanup
#[actix_rt::test]
async fn test_stale_cleanup() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    log::info!("=== Test: The Ghost (Stale Cleanup) ===");

    log::info!("=== Phase 1: Setup Database with 1ms Timeout ===");

    // 1. Start the Database Router
    let db_router = RouterActor::new(vec![]).start();

    // 2. Create Database with 1ms stale timeout for instant testing
    let db_config = CStateConfig::for_database(1, None); // 1 millisecond stale timeout!

    let mut db_permissions = HashMap::new();
    db_permissions.insert("collector".to_string(), collector_permissions());
    db_permissions.insert("admin".to_string(), admin_permissions());

    let db_actor = CStateBuilder::new(db_config)
        .router(db_router.clone())
        .permissions_map(db_permissions)
        .build();

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    log::info!("=== Phase 2: Register Zombie Collector ===");

    // 3. Create mock transport for Zombie collector
    let (trans_zombie_to_db, trans_db_to_zombie) = create_mock_pair("zombie_link");

    let zombie_conn = trans_db_to_zombie.into_established();
    let (db_zombie_tx, db_zombie_recv_rx, _watcher) = (zombie_conn.tx, zombie_conn.rx, zombie_conn.watcher);

    let db_routing_map = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("Zombie-1"),
            role: Role::new("collector"),
            negotiated_rooms: vec![RoomId::from(CSTATE_ROOM)],
            transport_tx: db_zombie_tx,
        })
        .await
        .expect("Router should respond")
        .expect("Connection should succeed");

    let db_room_recipient = db_routing_map
        .get(&RoomId::from(CSTATE_ROOM))
        .expect("cstate room should exist")
        .clone();

    let zombie_send_conn = trans_zombie_to_db.into_established();
    let (zombie_send_tx, mut zombie_recv_rx, _watcher) = (zombie_send_conn.tx, zombie_send_conn.rx, zombie_send_conn.watcher);

    // Route messages to database
    actix::spawn(async move {
        let mut rx = db_zombie_recv_rx;
        while let Some(result) = rx.recv().await {
            if let Ok(frame) = result
                && let Ok(inner) = Frame::deserialize(frame.get_bytes())
                && let Frame::Room(RoomFrame::Message { payload, .. }) = inner
            {
                db_room_recipient.do_send(zznet_api::InboundRoomPayload { payload });
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // 4. Send Heartbeat from Zombie
    let heartbeat = create_heartbeat("Zombie-1", 456);
    let transport_frame = create_transport_frame(&heartbeat);

    zombie_send_tx
        .send(transport_frame)
        .await
        .expect("Send should succeed");

    // Wait for HeartbeatAck
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    let response = zombie_recv_rx
        .recv()
        .await
        .expect("Should receive response");
    let response_frame = response.expect("Response should be Ok");
    let response_msg = unwrap_message(response_frame);

    match &response_msg {
        CStateMessage::HeartbeatAck { .. } => {
            log::info!("✓ Zombie-1 registered successfully");
        }
        other => panic!("Expected HeartbeatAck, got {:?}", other),
    }

    log::info!("=== Phase 3: Wait for Timeout and Trigger Cleanup ===");

    // 5. Wait for timeout to expire (1ms timeout + small margin)
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;

    // 6. Manually trigger cleanup
    db_actor.do_send(CleanupStaleCollectors);

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    log::info!("=== Phase 4: Verify Empty List ===");

    // 7. Setup admin to query
    let (trans_admin_to_db, trans_db_to_admin) = create_mock_pair("admin_link");

    let db_admin_conn = trans_db_to_admin.into_established();
    let (db_admin_tx, db_admin_recv_rx, _watcher) = (db_admin_conn.tx, db_admin_conn.rx, db_admin_conn.watcher);

    let db_admin_routing_map = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("admin"),
            role: Role::new("admin"),
            negotiated_rooms: vec![RoomId::from(CSTATE_ROOM)],
            transport_tx: db_admin_tx,
        })
        .await
        .expect("Router should respond")
        .expect("Connection should succeed");

    let db_admin_room_recipient = db_admin_routing_map
        .get(&RoomId::from(CSTATE_ROOM))
        .expect("cstate room should exist")
        .clone();

    let admin_conn = trans_admin_to_db.into_established();
    let (admin_send_tx, mut admin_recv_rx, _watcher) = (admin_conn.tx, admin_conn.rx, admin_conn.watcher);

    actix::spawn(async move {
        let mut rx = db_admin_recv_rx;
        while let Some(result) = rx.recv().await {
            if let Ok(frame) = result
                && let Ok(inner) = Frame::deserialize(frame.get_bytes())
                && let Frame::Room(RoomFrame::Message { payload, .. }) = inner
            {
                db_admin_room_recipient.do_send(zznet_api::InboundRoomPayload { payload });
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // 8. Query collectors
    let query = CStateMessage::QueryCollectors;
    let transport_frame = create_transport_frame(&query);

    admin_send_tx
        .send(transport_frame)
        .await
        .expect("Send should succeed");

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    let response = admin_recv_rx.recv().await.expect("Should receive response");
    let response_frame = response.expect("Response should be Ok");
    let response_msg = unwrap_message(response_frame);

    match response_msg {
        CStateMessage::CollectorList { collectors } => {
            assert!(
                collectors.is_empty(),
                "Collector list should be empty after cleanup"
            );
            log::info!("✓ Collector list is empty (Zombie-1 was cleaned up)");
        }
        other => panic!("Expected CollectorList, got {:?}", other),
    }

    log::info!("=== Test Complete ===");
    log::info!("Verified: Stale collectors are removed after timeout");
}

// ============================================================================
// Test 3: The Impostor (Authorization)
// ============================================================================

/// Test that unauthorized roles cannot register as collectors:
/// 1. Peer connects with viewer role (no permissions)
/// 2. Peer sends Heartbeat
/// 3. Database responds with Unauthorized
/// 4. Admin query returns empty list
///
/// This test verifies:
/// - Permission enforcement works
/// - Unauthorized peers get rejected
/// - No state pollution from unauthorized attempts
#[actix_rt::test]
async fn test_authorization() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    log::info!("=== Test: The Impostor (Authorization) ===");

    log::info!("=== Phase 1: Setup Database with Permissions ===");

    // 1. Start the Database Router
    let db_router = RouterActor::new(vec![]).start();

    // 2. Create Database with permissions
    let db_config = CStateConfig::for_database(10_000, None);

    let mut db_permissions = HashMap::new();
    db_permissions.insert("collector".to_string(), collector_permissions());
    db_permissions.insert("admin".to_string(), admin_permissions());
    // "viewer" role gets deny_all (no can_send_heartbeat)
    db_permissions.insert("viewer".to_string(), deny_all_permissions());

    let _db_actor = CStateBuilder::new(db_config)
        .router(db_router.clone())
        .permissions_map(db_permissions)
        .build();

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    log::info!("=== Phase 2: Impostor Attempts Registration ===");

    // 3. Create mock transport for spy
    let (trans_spy_to_db, trans_db_to_spy) = create_mock_pair("spy_link");

    let spy_conn = trans_db_to_spy.into_established();
    let (db_spy_tx, db_spy_recv_rx, _watcher) = (spy_conn.tx, spy_conn.rx, spy_conn.watcher);

    // Connect as "viewer" role (has no heartbeat permission)
    let db_routing_map = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("spy-01"),
            role: Role::new("viewer"),
            negotiated_rooms: vec![RoomId::from(CSTATE_ROOM)],
            transport_tx: db_spy_tx,
        })
        .await
        .expect("Router should respond")
        .expect("Connection should succeed");

    let db_room_recipient = db_routing_map
        .get(&RoomId::from(CSTATE_ROOM))
        .expect("cstate room should exist")
        .clone();

    let spy_send_conn = trans_spy_to_db.into_established();
    let (spy_send_tx, mut spy_recv_rx, _watcher) = (spy_send_conn.tx, spy_send_conn.rx, spy_send_conn.watcher);

    // Route messages to database
    actix::spawn(async move {
        let mut rx = db_spy_recv_rx;
        while let Some(result) = rx.recv().await {
            if let Ok(frame) = result
                && let Ok(inner) = Frame::deserialize(frame.get_bytes())
                && let Frame::Room(RoomFrame::Message { payload, .. }) = inner
            {
                db_room_recipient.do_send(zznet_api::InboundRoomPayload { payload });
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // 4. Send Heartbeat from spy (unauthorized)
    let heartbeat = create_heartbeat("spy-01", 789);
    let transport_frame = create_transport_frame(&heartbeat);

    spy_send_tx
        .send(transport_frame)
        .await
        .expect("Send should succeed");

    log::debug!("Spy sent Heartbeat");

    log::info!("=== Phase 3: Verify Rejection ===");

    // 5. Wait for Unauthorized response
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    let response = spy_recv_rx.recv().await.expect("Should receive response");
    let response_frame = response.expect("Response should be Ok");
    let response_msg = unwrap_message(response_frame);

    match &response_msg {
        CStateMessage::Unauthorized { reason } => {
            log::info!("✓ Received Unauthorized: {}", reason);
        }
        CStateMessage::HeartbeatAck { .. } => {
            panic!("Should NOT receive HeartbeatAck for unauthorized peer!");
        }
        other => panic!("Expected Unauthorized, got {:?}", other),
    }

    log::info!("=== Phase 4: Verify Empty Collector List ===");

    // 6. Setup admin to verify no state pollution
    let (trans_admin_to_db, trans_db_to_admin) = create_mock_pair("admin_link");

    let db_admin_conn = trans_db_to_admin.into_established();
    let (db_admin_tx, db_admin_recv_rx, _watcher) = (db_admin_conn.tx, db_admin_conn.rx, db_admin_conn.watcher);

    let db_admin_routing_map = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("admin"),
            role: Role::new("admin"),
            negotiated_rooms: vec![RoomId::from(CSTATE_ROOM)],
            transport_tx: db_admin_tx,
        })
        .await
        .expect("Router should respond")
        .expect("Connection should succeed");

    let db_admin_room_recipient = db_admin_routing_map
        .get(&RoomId::from(CSTATE_ROOM))
        .expect("cstate room should exist")
        .clone();

    let admin_conn = trans_admin_to_db.into_established();
    let (admin_send_tx, mut admin_recv_rx, _watcher) = (admin_conn.tx, admin_conn.rx, admin_conn.watcher);

    actix::spawn(async move {
        let mut rx = db_admin_recv_rx;
        while let Some(result) = rx.recv().await {
            if let Ok(frame) = result
                && let Ok(inner) = Frame::deserialize(frame.get_bytes())
                && let Frame::Room(RoomFrame::Message { payload, .. }) = inner
            {
                db_admin_room_recipient.do_send(zznet_api::InboundRoomPayload { payload });
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // 7. Query collectors
    let query = CStateMessage::QueryCollectors;
    let transport_frame = create_transport_frame(&query);

    admin_send_tx
        .send(transport_frame)
        .await
        .expect("Send should succeed");

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    let response = admin_recv_rx.recv().await.expect("Should receive response");
    let response_frame = response.expect("Response should be Ok");
    let response_msg = unwrap_message(response_frame);

    match response_msg {
        CStateMessage::CollectorList { collectors } => {
            assert!(
                collectors.is_empty(),
                "Collector list should be empty (spy was rejected)"
            );
            log::info!("✓ Collector list is empty (no state pollution from unauthorized attempt)");
        }
        other => panic!("Expected CollectorList, got {:?}", other),
    }

    log::info!("=== Test Complete ===");
    log::info!(
        "Verified: Unauthorized peers are rejected, no HeartbeatAck sent, no state pollution"
    );
}

// ============================================================================
// Test 4: The Capacity (Load Shedding)
// ============================================================================

/// Test that max_collectors limit is enforced:
/// 1. Database configured with max_collectors = 1
/// 2. Collector-1 registers successfully
/// 3. Collector-2 attempts to register
/// 4. Collector-2 receives RegistrationRejected
/// 5. Admin query shows only Collector-1
///
/// This test verifies:
/// - Capacity limit is enforced
/// - RegistrationRejected message is sent
/// - Only first collector is in the list
#[actix_rt::test]
async fn test_capacity_limit() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    log::info!("=== Test: The Capacity (Load Shedding) ===");

    log::info!("=== Phase 1: Setup Database with Capacity Limit ===");

    // 1. Start the Database Router
    let db_router = RouterActor::new(vec![]).start();

    // 2. Create Database with max_collectors = 1
    let db_config = CStateConfig::for_database(10_000, Some(1)); // max 1 collector

    let mut db_permissions = HashMap::new();
    db_permissions.insert("collector".to_string(), collector_permissions());
    db_permissions.insert("admin".to_string(), admin_permissions());

    let _db_actor = CStateBuilder::new(db_config)
        .router(db_router.clone())
        .permissions_map(db_permissions)
        .build();

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    log::info!("=== Phase 2: First Collector Registers ===");

    // 3. Connect Collector-1
    let (trans_coll1_to_db, trans_db_to_coll1) = create_mock_pair("coll1_link");

    let coll1_conn = trans_db_to_coll1.into_established();
    let (db_coll1_tx, db_coll1_recv_rx, _watcher) = (coll1_conn.tx, coll1_conn.rx, coll1_conn.watcher);

    let db_routing_map_1 = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("Coll-1"),
            role: Role::new("collector"),
            negotiated_rooms: vec![RoomId::from(CSTATE_ROOM)],
            transport_tx: db_coll1_tx,
        })
        .await
        .expect("Router should respond")
        .expect("Connection should succeed");

    let db_room_recipient_1 = db_routing_map_1
        .get(&RoomId::from(CSTATE_ROOM))
        .expect("cstate room should exist")
        .clone();

    let coll1_send_conn = trans_coll1_to_db.into_established();
    let (coll1_send_tx, mut coll1_recv_rx, _watcher) = (coll1_send_conn.tx, coll1_send_conn.rx, coll1_send_conn.watcher);

    actix::spawn(async move {
        let mut rx = db_coll1_recv_rx;
        while let Some(result) = rx.recv().await {
            if let Ok(frame) = result
                && let Ok(inner) = Frame::deserialize(frame.get_bytes())
                && let Frame::Room(RoomFrame::Message { payload, .. }) = inner
            {
                db_room_recipient_1.do_send(zznet_api::InboundRoomPayload { payload });
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // 4. Collector-1 sends Heartbeat
    let heartbeat = create_heartbeat("Coll-1", 111);
    let transport_frame = create_transport_frame(&heartbeat);

    coll1_send_tx
        .send(transport_frame)
        .await
        .expect("Send should succeed");

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    let response = coll1_recv_rx.recv().await.expect("Should receive response");
    let response_frame = response.expect("Response should be Ok");
    let response_msg = unwrap_message(response_frame);

    match &response_msg {
        CStateMessage::HeartbeatAck { .. } => {
            log::info!("✓ Coll-1 registered successfully");
        }
        other => panic!("Expected HeartbeatAck for Coll-1, got {:?}", other),
    }

    log::info!("=== Phase 3: Second Collector Attempts Registration ===");

    // 5. Connect Collector-2
    let (trans_coll2_to_db, trans_db_to_coll2) = create_mock_pair("coll2_link");

    let coll2_conn = trans_db_to_coll2.into_established();
    let (db_coll2_tx, db_coll2_recv_rx, _watcher) = (coll2_conn.tx, coll2_conn.rx, coll2_conn.watcher);

    let db_routing_map_2 = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("Coll-2"),
            role: Role::new("collector"),
            negotiated_rooms: vec![RoomId::from(CSTATE_ROOM)],
            transport_tx: db_coll2_tx,
        })
        .await
        .expect("Router should respond")
        .expect("Connection should succeed");

    let db_room_recipient_2 = db_routing_map_2
        .get(&RoomId::from(CSTATE_ROOM))
        .expect("cstate room should exist")
        .clone();

    let coll2_send_conn = trans_coll2_to_db.into_established();
    let (coll2_send_tx, mut coll2_recv_rx, _watcher) = (coll2_send_conn.tx, coll2_send_conn.rx, coll2_send_conn.watcher);

    actix::spawn(async move {
        let mut rx = db_coll2_recv_rx;
        while let Some(result) = rx.recv().await {
            if let Ok(frame) = result
                && let Ok(inner) = Frame::deserialize(frame.get_bytes())
                && let Frame::Room(RoomFrame::Message { payload, .. }) = inner
            {
                db_room_recipient_2.do_send(zznet_api::InboundRoomPayload { payload });
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // 6. Collector-2 sends Heartbeat (should be rejected)
    let heartbeat = create_heartbeat("Coll-2", 222);
    let transport_frame = create_transport_frame(&heartbeat);

    coll2_send_tx
        .send(transport_frame)
        .await
        .expect("Send should succeed");

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    let response = coll2_recv_rx.recv().await.expect("Should receive response");
    let response_frame = response.expect("Response should be Ok");
    let response_msg = unwrap_message(response_frame);

    match &response_msg {
        CStateMessage::RegistrationRejected { reason } => {
            log::info!("✓ Coll-2 rejected: {}", reason);
            assert!(
                reason.contains("capacity"),
                "Reason should mention capacity"
            );
        }
        CStateMessage::HeartbeatAck { .. } => {
            panic!("Should NOT receive HeartbeatAck when at capacity!");
        }
        other => panic!("Expected RegistrationRejected for Coll-2, got {:?}", other),
    }

    log::info!("=== Phase 4: Verify Only Coll-1 in List ===");

    // 7. Setup admin to query
    let (trans_admin_to_db, trans_db_to_admin) = create_mock_pair("admin_link");

    let db_admin_conn = trans_db_to_admin.into_established();
    let (db_admin_tx, db_admin_recv_rx, _watcher) = (db_admin_conn.tx, db_admin_conn.rx, db_admin_conn.watcher);

    let db_admin_routing_map = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("admin"),
            role: Role::new("admin"),
            negotiated_rooms: vec![RoomId::from(CSTATE_ROOM)],
            transport_tx: db_admin_tx,
        })
        .await
        .expect("Router should respond")
        .expect("Connection should succeed");

    let db_admin_room_recipient = db_admin_routing_map
        .get(&RoomId::from(CSTATE_ROOM))
        .expect("cstate room should exist")
        .clone();

    let admin_send_conn = trans_admin_to_db.into_established();
    let (admin_send_tx, mut admin_recv_rx, _watcher) = (admin_send_conn.tx, admin_send_conn.rx, admin_send_conn.watcher);

    actix::spawn(async move {
        let mut rx = db_admin_recv_rx;
        while let Some(result) = rx.recv().await {
            if let Ok(frame) = result
                && let Ok(inner) = Frame::deserialize(frame.get_bytes())
                && let Frame::Room(RoomFrame::Message { payload, .. }) = inner
            {
                db_admin_room_recipient.do_send(zznet_api::InboundRoomPayload { payload });
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // 8. Query collectors
    let query = CStateMessage::QueryCollectors;
    let transport_frame = create_transport_frame(&query);

    admin_send_tx
        .send(transport_frame)
        .await
        .expect("Send should succeed");

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    let response = admin_recv_rx.recv().await.expect("Should receive response");
    let response_frame = response.expect("Response should be Ok");
    let response_msg = unwrap_message(response_frame);

    match response_msg {
        CStateMessage::CollectorList { collectors } => {
            assert_eq!(collectors.len(), 1, "Should have exactly 1 collector");
            assert_eq!(
                collectors[0].id, "Coll-1",
                "Only Coll-1 should be registered"
            );
            log::info!(
                "✓ Collector list contains only Coll-1: {:?}",
                collectors.iter().map(|c| &c.id).collect::<Vec<_>>()
            );
        }
        other => panic!("Expected CollectorList, got {:?}", other),
    }

    log::info!("=== Test Complete ===");
    log::info!(
        "Verified: Capacity limit enforced, RegistrationRejected sent, only first collector registered"
    );
}
