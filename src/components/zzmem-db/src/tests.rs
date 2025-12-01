//! Tests for the zzmem-db component
//!
//! These tests verify the "Storage & Buffering" pattern:
//! - Collector buffers results locally
//! - Collector sends batches to Database
//! - Database stores and responds to queries
//! - Backpressure handling during network outages
//!
//! # Architecture: "Sociable Unit Test"
//!
//! Real actors talking via mocked transport. This pattern tests the full
//! three-actor system (MainActor + NetworkManager + NetworkActor per peer)
//! with actual message serialization and routing.

use crate::builder::MemDBBuilder;
use crate::config::MemDBConfig;
use crate::messages::{GetHealth, StorePingResult};
use crate::network_messages::MemDBMessage;
use crate::permissions::MemDBPermissions;
use crate::types::{PingResult, PingStatus};
use actix::prelude::*;
use std::collections::HashMap;
use std::time::Duration;
use zznet_api::{Frame, OnPeerConnected, PeerId, Role, RoomFrame, RoomId, create_mock_pair};
use zznet_router::RouterActor;
use zzstorage::actor::{GetStoredBlobs, StorageActor, StorageConfig};

// ============================================================================
// Helper Functions
// ============================================================================

/// Create permissions for a collector role (can submit batches)
fn collector_permissions() -> MemDBPermissions {
    MemDBPermissions::collector()
}

/// Create permissions for a database role (can receive batches and query)
fn database_permissions() -> MemDBPermissions {
    MemDBPermissions::database()
}

/// Helper to create a PingResult with specified target and timestamp
fn create_ping_result(target: &str, time_offset_ms: u64, rtt_us: Option<u32>) -> PingResult {
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    PingResult {
        target: target.to_string(),
        sent_time_ns: now_ns + (time_offset_ms * 1_000_000),
        status: match rtt_us {
            Some(us) => PingStatus::Success(us as u64 * 1000),
            None => PingStatus::Timeout,
        },
    }
}

// ============================================================================
// Test 1: Rewind and Flush-to-Storage
// ============================================================================

#[actix_rt::test]
async fn test_rewind_and_flush() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .init();

    // 1. Setup Mock Storage Actor
    let storage_actor = StorageActor::new(StorageConfig::Ephemeral).start();

    // 2. Setup Database MemDB
    let db_router = RouterActor::new(vec![]).start();
    let db_config = MemDBConfig::for_database(100, None);
    let mut db_permissions = HashMap::new();
    db_permissions.insert("collector".to_string(), collector_permissions());
    let db_actor = MemDBBuilder::new(db_config)
        .router(db_router.clone())
        .permissions_map(db_permissions)
        .with_storage_actor(storage_actor.clone()) // Inject mock storage
        .build();

    // 3. Setup Collector MemDB
    let coll_router = RouterActor::new(vec![]).start();
    let coll_config = MemDBConfig::for_collector(10);
    let mut coll_permissions = HashMap::new();
    coll_permissions.insert("database".to_string(), database_permissions());
    let coll_actor = MemDBBuilder::new(coll_config)
        .router(coll_router.clone())
        .permissions_map(coll_permissions)
        .build();

    // 4. Send some data to the collector *before* it connects
    for i in 0..5 {
        coll_actor.do_send(StorePingResult {
            result: create_ping_result("8.8.8.8", i, Some(10)),
        });
    }
    tokio::time::sleep(Duration::from_millis(1)).await;
    let health = coll_actor.send(GetHealth).await.unwrap().unwrap();
    assert_eq!(health.buffer_size, 5);

    // 5. Connect Collector to Database
    let (trans_coll_to_db, trans_db_to_coll) = create_mock_pair("link");
    let db_conn = trans_db_to_coll.into_established();
    let coll_conn = trans_coll_to_db.into_established();

    let db_routing_map = db_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("collector-01"),
            role: Role::new("collector"),
            negotiated_rooms: vec![RoomId::from("memdb")],
            transport_tx: db_conn.tx.clone(),
        })
        .await
        .unwrap()
        .unwrap();
    let _db_room_recipient = db_routing_map.get(&RoomId::from("memdb")).unwrap().clone();

    let coll_routing_map = coll_router
        .send(OnPeerConnected {
            peer_id: PeerId::new("database"),
            role: Role::new("database"),
            negotiated_rooms: vec![RoomId::from("memdb")],
            transport_tx: coll_conn.tx.clone(),
        })
        .await
        .unwrap()
        .unwrap();
    let _coll_room_recipient = coll_routing_map
        .get(&RoomId::from("memdb"))
        .unwrap()
        .clone();

    let mut db_rx = db_conn.rx;
    let db_actor_clone = db_actor.clone();
    actix::spawn(async move {
        while let Some(Ok(frame)) = db_rx.recv().await {
            let inner_frame = Frame::deserialize(frame.get_bytes()).unwrap();
            if let Frame::Room(RoomFrame::Message { payload, .. }) = inner_frame {
                let msg: MemDBMessage = rmp_serde::from_slice(&payload).unwrap();
                if let MemDBMessage::SubmitBatch {
                    peer_id,
                    timestamp_ms,
                    results,
                } = msg
                {
                    db_actor_clone.do_send(crate::internal_messages::InboundSubmitBatch {
                        peer_id,
                        timestamp_ms,
                        results,
                    });
                }
            }
        }
    });

    let mut coll_rx = coll_conn.rx;
    let coll_actor_clone = coll_actor.clone();
    actix::spawn(async move {
        while let Some(Ok(frame)) = coll_rx.recv().await {
            let inner_frame = Frame::deserialize(frame.get_bytes()).unwrap();
            if let Frame::Room(RoomFrame::Message { payload, .. }) = inner_frame {
                let msg: MemDBMessage = rmp_serde::from_slice(&payload).unwrap();
                match msg {
                    MemDBMessage::HelloCollector { last_persisted_ts } => {
                        coll_actor_clone.do_send(crate::internal_messages::InboundHelloCollector {
                            last_persisted_ts,
                        });
                    }
                    MemDBMessage::BatchAck {
                        peer_id,
                        received_count,
                        timestamp_ms,
                    } => {
                        coll_actor_clone.do_send(crate::internal_messages::InboundBatchAck {
                            peer_id,
                            received_count,
                            timestamp_ms,
                        });
                    }
                    _ => {}
                }
            }
        }
    });

    // 6. Wait for HelloCollector and rewind
    tokio::time::sleep(Duration::from_millis(10)).await;

    // 7. Send more data to trigger a batch send
    for i in 5..10 {
        coll_actor.do_send(StorePingResult {
            result: create_ping_result("8.8.8.8", i, Some(10)),
        });
    }

    // 8. Wait for batch to be processed by DB
    tokio::time::sleep(Duration::from_millis(50)).await; // Allow for async message propagation
    let db_health = db_actor.send(GetHealth).await.unwrap().unwrap();
    log::info!("DB health after batch: {:?}", db_health);

    // 9. Trigger a flush on the DB side
    db_actor.do_send(crate::internal_messages::FlushToStorage);
    tokio::time::sleep(Duration::from_millis(10)).await;

    // 10. Verify data made it to the storage actor
    let stored_blobs = storage_actor.send(GetStoredBlobs).await.unwrap().unwrap();
    log::info!("Stored blobs: {:?}", stored_blobs.len());
    assert_eq!(stored_blobs.len(), 1);
}
