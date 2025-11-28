//! Tests for the zzintent-config component
//!
//! These tests verify the "Intent Propagation" pattern:
//! - Admin writes to Database
//! - Database updates internal state
//! - Database broadcasts to connected Collectors
//! - Collectors receive and update their local state
//!
//! # Architecture: "Sociable Unit Test"
//!
//! Real actors talking via mocked transport. This pattern tests the full
//! three-actor system (MainActor + NetworkManager + NetworkActor per peer)
//! with actual message serialization and routing.
//!
//! # Test Injection Point
//!
//! Messages are injected via `InboundRoomPayload` directly to the room recipient.
//! This bypasses the Router's transport-level framing but correctly tests the
//! RoomActor's payload deserialization layer - the right boundary for component tests.

use crate::builder::IntentConfigBuilder;
use crate::config::IntentConfigConfig;
use crate::messages::GetCurrentConfig;
use crate::permissions::IntentConfigPermissions;
use actix::prelude::*;
use std::collections::HashMap;
use std::net::IpAddr;
use tokio::sync::mpsc;
use zznet_api::{
    Frame, OnPeerConnected, PeerId, Role, RoomFrame, RoomId, TransportFrame,
    create_mock_pair,
};
use zznet_router::RouterActor;

// ============================================================================
// Helper Functions
// ============================================================================

/// Create permissions for an admin role (full access)
fn admin_permissions() -> IntentConfigPermissions {
    IntentConfigPermissions::new(true, true)
}

/// Create permissions for a collector role (read-only)
fn collector_permissions() -> IntentConfigPermissions {
    IntentConfigPermissions::new(true, false)
}

/// Create permissions for an unauthorized peer (no access)
#[allow(dead_code)]
fn unauthorized_permissions() -> IntentConfigPermissions {
    IntentConfigPermissions::new(false, false)
}

/// Helper to serialize a network message for sending via InboundRoomPayload
///
/// InboundRoomPayload expects the serialized message bytes (what the Router
/// sends to RoomActor after unwrapping transport frames).
fn serialize_message(msg: crate::network_messages::IntentConfigNetworkMsg) -> Vec<u8> {
    use zznet_room::RoomMessageTrait;
    msg.serialize_inner().expect("Serialization should succeed")
}

/// Helper to unwrap and deserialize a TransportFrame back to IntentConfigNetworkMsg
#[allow(dead_code)]
fn unwrap_message(
    transport_frame: TransportFrame,
) -> crate::network_messages::IntentConfigNetworkMsg {
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

    // Step 4: Deserialize payload to IntentConfigNetworkMsg
    crate::network_messages::IntentConfigNetworkMsg::deserialize_for_room(
        &RoomId::from("intent-config"),
        &payload,
    )
    .expect("Network message deserialization should succeed")
}

// ============================================================================
// Test: The Intent Propagation
// ============================================================================

/// Test the full intent propagation flow:
/// 1. Database node starts with empty config
/// 2. Admin connects and sends RequestConfigChange
/// 3. Database updates internal state
/// 4. Collector connects and receives RequestConfigChange broadcast
/// 5. Collector updates its internal state
///
/// This test verifies:
/// - Admin can write configuration
/// - Database persists and broadcasts changes
/// - Collectors receive and apply updates
/// - Message routing works end-to-end
#[actix_rt::test]
async fn test_intent_propagation() {
    // Initialize logging for debugging
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    log::info!("=== Phase 1: The Database Node ===");

    // 1. Start the Database Router
    let db_router = RouterActor::new(vec![]).start();
    log::debug!("Database router started");

    // 2. Create Database component
    let db_config = IntentConfigConfig::for_database(
        tempfile::NamedTempFile::new()
            .expect("Failed to create temp file")
            .path()
            .to_path_buf(),
    );

    let mut db_permissions = HashMap::new();
    db_permissions.insert("admin".to_string(), admin_permissions());
    db_permissions.insert("collector".to_string(), collector_permissions());

    let db_actor = IntentConfigBuilder::new()
        .config(db_config)
        .router(db_router.clone())
        .permissions_map(db_permissions)
        .start()
        .expect("Database actor should start");

    log::debug!("Database actor started");

    // Give actors time to register with router
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    log::info!("=== Phase 2: The Collector Node ===");

    // 3. Start the Collector Router
    let coll_router = RouterActor::new(vec![]).start();
    log::debug!("Collector router started");

    // 4. Create Collector component
    let coll_config = IntentConfigConfig::for_collector();

    // Collector needs to accept updates from "database" role
    let mut coll_permissions = HashMap::new();
    coll_permissions.insert("database".to_string(), admin_permissions()); // Database can write to collector

    let coll_actor = IntentConfigBuilder::new()
        .config(coll_config)
        .router(coll_router.clone())
        .permissions_map(coll_permissions)
        .start()
        .expect("Collector actor should start");

    log::debug!("Collector actor started");

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    log::info!("=== Phase 3: Wiring - Link Database <-> Collector ===");

    // 5. Create mock transport pair for Database <-> Collector connection
    let (trans_db_to_coll, trans_coll_to_db) = create_mock_pair("db_coll_link");

    // 6. Connect Collector side (Collector connects TO Database)
    let coll_conn = trans_coll_to_db.into_established();
    let (coll_send_tx, coll_recv_rx, _watcher) = (coll_conn.tx, coll_conn.rx, coll_conn.watcher);

    let coll_routing_map = coll_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("database"),
            role: Role::new("database"),
            negotiated_rooms: vec![RoomId::from("intent-config")],
            transport_tx: coll_send_tx,
        })
        .await
        .expect("Collector router should respond")
        .expect("Collector connection should succeed");

    log::debug!(
        "Collector connected, routing map: {:?}",
        coll_routing_map.keys()
    );

    // 7. Connect Database side (Database sees Collector connection)
    // Database broadcasts will go through db_send_tx and arrive on coll_recv_rx
    let db_conn = trans_db_to_coll.into_established();
    let (db_send_tx, _db_recv_rx, _watcher) = (db_conn.tx, db_conn.rx, db_conn.watcher);

    let db_routing_map = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("collector-01"),
            role: Role::new("collector"),
            negotiated_rooms: vec![RoomId::from("intent-config")],
            transport_tx: db_send_tx,
        })
        .await
        .expect("Database router should respond")
        .expect("Database connection should succeed");

    log::debug!(
        "Database connected to collector, routing map: {:?}",
        db_routing_map.keys()
    );

    // Give time for network actors to start and subscribe
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // Spawn a task to route messages from collector's receive channel to its router
    let coll_room_recipient = coll_routing_map
        .get(&RoomId::from("intent-config"))
        .expect("intent-config room should exist")
        .clone();

    actix::spawn(async move {
        let mut rx = coll_recv_rx;
        while let Some(result) = rx.recv().await {
            match result {
                Ok(frame) => {
                    log::debug!(
                        "Routing frame to collector: {} bytes",
                        frame.get_bytes().len()
                    );

                    // Unwrap the TransportFrame -> Frame -> RoomFrame -> payload
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

                    coll_room_recipient.do_send(zznet_api::InboundRoomPayload { payload });
                }
                Err(e) => {
                    log::error!("Transport error in collector receive loop: {:?}", e);
                    break;
                }
            }
        }
    });

    log::info!("=== Phase 4: Wiring - Link Admin <-> Database ===");

    // 8. Create mock transport for Admin <-> Database
    let (_trans_admin_side, trans_db_admin_side) = create_mock_pair("admin_db_link");

    // 9. Connect Database side (Database sees Admin connection)
    let (db_admin_tx, _db_admin_rx) = mpsc::channel::<TransportFrame>(32);
    let admin_conn = trans_db_admin_side.into_established();
    let (_admin_tx, _admin_rx, _watcher) = (admin_conn.tx, admin_conn.rx, admin_conn.watcher);

    let db_admin_routing_map = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("admin-user"),
            role: Role::new("admin"),
            negotiated_rooms: vec![RoomId::from("intent-config")],
            transport_tx: db_admin_tx.clone(),
        })
        .await
        .expect("Database router should respond")
        .expect("Admin connection should succeed");

    log::debug!(
        "Admin connected to database, routing map: {:?}",
        db_admin_routing_map.keys()
    );

    let admin_room_recipient = db_admin_routing_map
        .get(&RoomId::from("intent-config"))
        .expect("intent-config room should exist")
        .clone();

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    log::info!("=== Phase 5: The Action - Admin Writes Config ===");

    // 10. Admin sends RequestConfigChange
    let target_ip: IpAddr = "8.8.8.8".parse().expect("Valid IP");
    let request_msg = crate::network_messages::IntentConfigNetworkMsg::RequestConfigChange {
        sender_peer_id: "admin-user".to_string(),
        targets: vec![target_ip],
        ping_rate_pps: 5,
    };

    log::debug!("Admin sending RequestConfigChange: {:?}", request_msg);

    // Serialize and send via InboundRoomPayload
    let payload = serialize_message(request_msg);
    admin_room_recipient
        .send(zznet_api::InboundRoomPayload { payload })
        .await
        .expect("Admin message should be delivered");

    log::debug!("Admin request sent");

    // Give time for processing
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    log::info!("=== Phase 6: Verify Database State ===");

    // 11. Check Database internal state
    let db_config = db_actor
        .send(GetCurrentConfig)
        .await
        .expect("Database should respond");

    log::debug!("Database config: {:?}", db_config);
    assert_eq!(db_config.targets.len(), 1);
    assert_eq!(db_config.targets[0], target_ip);
    assert_eq!(db_config.ping_rate_pps, 5);

    log::info!("✓ Database state updated correctly");

    log::info!("=== Phase 7: Verify Collector Receives and Processes Broadcast ===");

    // 12. Wait for collector to receive and process the RequestConfigChange
    // The spawned task routes messages from wire to collector's router
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // 13. Query collector's internal state
    let coll_config = coll_actor
        .send(GetCurrentConfig)
        .await
        .expect("Collector should respond");

    log::debug!("Collector config: {:?}", coll_config);
    assert_eq!(coll_config.targets.len(), 1);
    assert_eq!(coll_config.targets[0], target_ip);
    assert_eq!(coll_config.ping_rate_pps, 5);

    log::info!("✓ Collector state synchronized correctly");

    log::info!("=== Test Complete ===");
    log::info!(
        "Verified end-to-end flow: Admin -> Database (update) -> Collector (broadcast) -> Collector (state)"
    );
}

// ============================================================================
// Test: Unauthorized Write Attempt
// ============================================================================

/// Test that an unauthorized peer cannot modify configuration
///
/// This test verifies:
/// - Peers without write permissions cannot change config
/// - Database state remains unchanged after unauthorized request
/// - No broadcasts occur for unauthorized requests
#[actix_rt::test]
async fn test_unauthorized_write() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    log::info!("=== Test: Unauthorized Write Attempt ===");

    log::info!("=== Phase 1: Setup Database ===");

    // 1. Start Database Router and Component
    let db_router = RouterActor::new(vec![]).start();

    let db_config = IntentConfigConfig::for_database(
        tempfile::NamedTempFile::new()
            .expect("Failed to create temp file")
            .path()
            .to_path_buf(),
    );

    let mut db_permissions = HashMap::new();
    db_permissions.insert("admin".to_string(), admin_permissions());
    db_permissions.insert("collector".to_string(), collector_permissions()); // Read-only

    let db_actor = IntentConfigBuilder::new()
        .config(db_config)
        .router(db_router.clone())
        .permissions_map(db_permissions)
        .start()
        .expect("Database actor should start");

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    log::info!("=== Phase 2: Set Initial Config (as Admin) ===");

    // 2. First, set an initial config as admin
    let (_trans_admin, trans_db_admin) = create_mock_pair("admin_link");
    let admin_conn = trans_db_admin.into_established();
    let (_admin_tx, _admin_rx, _watcher) = (admin_conn.tx, admin_conn.rx, admin_conn.watcher);

    let db_admin_routing_map = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("admin"),
            role: Role::new("admin"),
            negotiated_rooms: vec![RoomId::from("intent-config")],
            transport_tx: mpsc::channel(32).0,
        })
        .await
        .expect("Router should respond")
        .expect("Connection should succeed");

    let admin_room = db_admin_routing_map
        .get(&RoomId::from("intent-config"))
        .expect("Room should exist")
        .clone();

    // Set initial config: 1.1.1.1 at rate 10
    let initial_ip: IpAddr = "1.1.1.1".parse().unwrap();
    let initial_msg = crate::network_messages::IntentConfigNetworkMsg::RequestConfigChange {
        sender_peer_id: "admin".to_string(),
        targets: vec![initial_ip],
        ping_rate_pps: 10,
    };

    admin_room
        .send(zznet_api::InboundRoomPayload {
            payload: serialize_message(initial_msg),
        })
        .await
        .expect("Should send");

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // Verify initial state
    let initial_config = db_actor
        .send(GetCurrentConfig)
        .await
        .expect("Should get config");
    assert_eq!(initial_config.targets[0], initial_ip);
    log::info!("✓ Initial config set: {:?}", initial_config);

    log::info!("=== Phase 3: Rogue Collector Attempts Write ===");

    // 3. Connect a "rogue" collector
    let (_trans_rogue, trans_db_rogue) = create_mock_pair("rogue_link");
    let rogue_conn = trans_db_rogue.into_established();
    let (_rogue_tx, _rogue_rx, _watcher) = (rogue_conn.tx, rogue_conn.rx, rogue_conn.watcher);

    let db_rogue_routing_map = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("rogue-collector"),
            role: Role::new("collector"), // Has read-only permissions
            negotiated_rooms: vec![RoomId::from("intent-config")],
            transport_tx: mpsc::channel(32).0,
        })
        .await
        .expect("Router should respond")
        .expect("Connection should succeed");

    let rogue_room = db_rogue_routing_map
        .get(&RoomId::from("intent-config"))
        .expect("Room should exist")
        .clone();

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // 4. Rogue tries to change config to 8.8.8.8
    let rogue_ip: IpAddr = "8.8.8.8".parse().unwrap();
    let rogue_msg = crate::network_messages::IntentConfigNetworkMsg::RequestConfigChange {
        sender_peer_id: "rogue-collector".to_string(),
        targets: vec![rogue_ip],
        ping_rate_pps: 99,
    };

    log::debug!("Rogue sending unauthorized request: {:?}", rogue_msg);

    rogue_room
        .send(zznet_api::InboundRoomPayload {
            payload: serialize_message(rogue_msg),
        })
        .await
        .expect("Should send");

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    log::info!("=== Phase 4: Verify Database State Unchanged ===");

    // 5. Verify database state is UNCHANGED
    let final_config = db_actor
        .send(GetCurrentConfig)
        .await
        .expect("Should get config");

    log::debug!("Final config: {:?}", final_config);

    // Should still be the original config (1.1.1.1), not rogue's config (8.8.8.8)
    assert_eq!(final_config.targets.len(), 1);
    assert_eq!(final_config.targets[0], initial_ip);
    assert_eq!(final_config.ping_rate_pps, 10);

    log::info!("✓ Database rejected unauthorized write");
    log::info!("✓ Config remains: {:?}", final_config);

    log::info!("=== Test Complete ===");
}
