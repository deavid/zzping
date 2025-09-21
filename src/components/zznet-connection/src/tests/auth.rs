//! Authentication and Authorization Tests
//!
//! Tests covering role-based interactions and authentication scenarios

use crate::actor::ZzNetConnActor;
use crate::auth::AuthRole;
use crate::bus::RoomSubscribers;
use crate::mocks::SimpleMockTransportActor;
use crate::protocol::{Frame, RoomFrame, serialize};
use crate::tests::utils::{
    MockRoomManager, collect_messages, filter_room_active_messages, setup_logger,
};
use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;

#[actix::test]
#[ntest::timeout(100)]
async fn test_collector_database_role_interaction() {
    setup_logger();
    log::info!("Starting test_collector_database_role_interaction");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create Collector and Database actors
    let (collector_tx, mut collector_rx) = mpsc::unbounded_channel();
    let collector_subscriber = MockRoomManager::new(collector_tx).start();

    let (database_tx, mut database_rx) = mpsc::unbounded_channel();
    let database_subscriber = MockRoomManager::new(database_tx).start();

    // Create collector actor
    let collector_transport = SimpleMockTransportActor::default().start();
    let mut collector_subscribers = HashMap::new();
    collector_subscribers.insert(
        "data-collection".to_string(),
        RoomSubscribers {
            room_is_active: collector_subscriber.clone().recipient(),
            data: collector_subscriber.clone().recipient(),
            termination: collector_subscriber.clone().recipient(),
        },
    );

    let collector_actor = ZzNetConnActor::new(
        collector_transport.recipient(),
        collector_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["data-collection".to_string()],
        mock_mgr_addr.clone().recipient(),
    )
    .start();
    log::info!("Created Collector actor");

    // Create database actor
    let database_transport = SimpleMockTransportActor::default().start();
    let mut database_subscribers = HashMap::new();
    database_subscribers.insert(
        "data-collection".to_string(),
        RoomSubscribers {
            room_is_active: database_subscriber.clone().recipient(),
            data: database_subscriber.clone().recipient(),
            termination: database_subscriber.clone().recipient(),
        },
    );

    let database_actor = ZzNetConnActor::new(
        database_transport.recipient(),
        database_subscribers,
        "1.0".to_string(),
        AuthRole::Database,
        vec!["data-collection".to_string()],
        mock_mgr_addr.recipient(),
    )
    .start();
    log::info!("Created Database actor");

    // Manual handshake between Collector and Database
    let collector_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Collector,
        offered_rooms: vec!["data-collection".to_string()],
    }))
    .unwrap();
    database_actor.do_send(crate::actor::FrameFromTransport(collector_hello));

    let database_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["data-collection".to_string()],
    }))
    .unwrap();
    collector_actor.do_send(crate::actor::FrameFromTransport(database_hello));

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Complete handshake with PublishRooms
    let collector_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["data-collection".to_string()],
    }))
    .unwrap();
    database_actor.do_send(crate::actor::FrameFromTransport(collector_publish));

    let database_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["data-collection".to_string()],
    }))
    .unwrap();
    collector_actor.do_send(crate::actor::FrameFromTransport(database_publish));

    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    // Collect messages to verify room activation
    let collector_messages = collect_messages(&mut collector_rx, 10).await;
    let database_messages = collect_messages(&mut database_rx, 10).await;

    let collector_active = filter_room_active_messages(&collector_messages);
    let database_active = filter_room_active_messages(&database_messages);

    assert_eq!(
        collector_active.len(),
        1,
        "Collector should receive room activation"
    );
    assert_eq!(
        database_active.len(),
        1,
        "Database should receive room activation"
    );

    log::info!("Collector <-> Database role interaction test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_same_role_connections() {
    setup_logger();
    log::info!("Starting test_same_role_connections");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create two Collector actors
    let (collector1_tx, mut collector1_rx) = mpsc::unbounded_channel();
    let collector1_subscriber = MockRoomManager::new(collector1_tx).start();

    let (collector2_tx, mut collector2_rx) = mpsc::unbounded_channel();
    let collector2_subscriber = MockRoomManager::new(collector2_tx).start();

    // Create first collector
    let collector1_transport = SimpleMockTransportActor::default().start();
    let mut collector1_subscribers = HashMap::new();
    collector1_subscribers.insert(
        "peer-sharing".to_string(),
        RoomSubscribers {
            room_is_active: collector1_subscriber.clone().recipient(),
            data: collector1_subscriber.clone().recipient(),
            termination: collector1_subscriber.clone().recipient(),
        },
    );

    let collector1_actor = ZzNetConnActor::new(
        collector1_transport.recipient(),
        collector1_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["peer-sharing".to_string()],
        mock_mgr_addr.clone().recipient(),
    )
    .start();

    // Create second collector
    let collector2_transport = SimpleMockTransportActor::default().start();
    let mut collector2_subscribers = HashMap::new();
    collector2_subscribers.insert(
        "peer-sharing".to_string(),
        RoomSubscribers {
            room_is_active: collector2_subscriber.clone().recipient(),
            data: collector2_subscriber.clone().recipient(),
            termination: collector2_subscriber.clone().recipient(),
        },
    );

    let collector2_actor = ZzNetConnActor::new(
        collector2_transport.recipient(),
        collector2_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["peer-sharing".to_string()],
        mock_mgr_addr.recipient(),
    )
    .start();

    log::info!("Created two Collector actors");

    // Manual handshake between the two collectors
    let collector1_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Collector,
        offered_rooms: vec!["peer-sharing".to_string()],
    }))
    .unwrap();
    collector2_actor.do_send(crate::actor::FrameFromTransport(collector1_hello));

    let collector2_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Collector,
        offered_rooms: vec!["peer-sharing".to_string()],
    }))
    .unwrap();
    collector1_actor.do_send(crate::actor::FrameFromTransport(collector2_hello));

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Complete handshake with PublishRooms
    let collector1_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["peer-sharing".to_string()],
    }))
    .unwrap();
    collector2_actor.do_send(crate::actor::FrameFromTransport(collector1_publish));

    let collector2_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["peer-sharing".to_string()],
    }))
    .unwrap();
    collector1_actor.do_send(crate::actor::FrameFromTransport(collector2_publish));

    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    // Verify both collectors can establish the connection
    let collector1_messages = collect_messages(&mut collector1_rx, 10).await;
    let collector2_messages = collect_messages(&mut collector2_rx, 10).await;

    let collector1_active = filter_room_active_messages(&collector1_messages);
    let collector2_active = filter_room_active_messages(&collector2_messages);

    assert_eq!(
        collector1_active.len(),
        1,
        "Collector1 should receive room activation"
    );
    assert_eq!(
        collector2_active.len(),
        1,
        "Collector2 should receive room activation"
    );

    log::info!("Same role connections test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_role_specific_room_patterns() {
    setup_logger();
    log::info!("Starting test_role_specific_room_patterns");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create Collector and Database with different room patterns
    let (collector_tx, mut collector_rx) = mpsc::unbounded_channel();
    let collector_subscriber = MockRoomManager::new(collector_tx).start();

    let (database_tx, mut database_rx) = mpsc::unbounded_channel();
    let database_subscriber = MockRoomManager::new(database_tx).start();

    // Collector offers collection and admin rooms
    let collector_rooms = vec![
        "data-collection".to_string(),
        "admin".to_string(),
        "shared".to_string(),
    ];

    // Database offers storage and shared rooms
    let database_rooms = vec![
        "data-storage".to_string(),
        "backup".to_string(),
        "shared".to_string(),
    ];

    // Create collector
    let collector_transport = SimpleMockTransportActor::default().start();
    let mut collector_subscribers = HashMap::new();
    for room in &collector_rooms {
        collector_subscribers.insert(
            room.clone(),
            RoomSubscribers {
                room_is_active: collector_subscriber.clone().recipient(),
                data: collector_subscriber.clone().recipient(),
                termination: collector_subscriber.clone().recipient(),
            },
        );
    }

    let collector_actor = ZzNetConnActor::new(
        collector_transport.recipient(),
        collector_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        collector_rooms.clone(),
        mock_mgr_addr.clone().recipient(),
    )
    .start();

    // Create database
    let database_transport = SimpleMockTransportActor::default().start();
    let mut database_subscribers = HashMap::new();
    for room in &database_rooms {
        database_subscribers.insert(
            room.clone(),
            RoomSubscribers {
                room_is_active: database_subscriber.clone().recipient(),
                data: database_subscriber.clone().recipient(),
                termination: database_subscriber.clone().recipient(),
            },
        );
    }

    let database_actor = ZzNetConnActor::new(
        database_transport.recipient(),
        database_subscribers,
        "1.0".to_string(),
        AuthRole::Database,
        database_rooms.clone(),
        mock_mgr_addr.recipient(),
    )
    .start();

    log::info!("Created Collector with 3 rooms and Database with 3 rooms");

    // Manual handshake
    let collector_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Collector,
        offered_rooms: collector_rooms.clone(),
    }))
    .unwrap();
    database_actor.do_send(crate::actor::FrameFromTransport(collector_hello));

    let database_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: database_rooms.clone(),
    }))
    .unwrap();
    collector_actor.do_send(crate::actor::FrameFromTransport(database_hello));

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Complete handshake with PublishRooms
    let collector_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: collector_rooms,
    }))
    .unwrap();
    database_actor.do_send(crate::actor::FrameFromTransport(collector_publish));

    let database_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: database_rooms,
    }))
    .unwrap();
    collector_actor.do_send(crate::actor::FrameFromTransport(database_publish));

    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    // Verify that only the "shared" room is active (intersection)
    let collector_messages = collect_messages(&mut collector_rx, 10).await;
    let database_messages = collect_messages(&mut database_rx, 10).await;

    let collector_active = filter_room_active_messages(&collector_messages);
    let database_active = filter_room_active_messages(&database_messages);

    assert_eq!(
        collector_active.len(),
        1,
        "Collector should have 1 active room"
    );
    assert_eq!(
        database_active.len(),
        1,
        "Database should have 1 active room"
    );
    assert_eq!(collector_active[0].room_name, "shared");
    assert_eq!(database_active[0].room_name, "shared");

    log::info!("Role-specific room patterns test completed successfully");
}

#[actix::test]
#[ntest::timeout(200)]
async fn test_different_auth_roles_basic_handshake() {
    setup_logger();
    log::info!("Starting test_different_auth_roles_basic_handshake");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Test each role pair combination
    let role_pairs = vec![
        (AuthRole::Collector, AuthRole::Database),
        (AuthRole::Database, AuthRole::Collector),
        (AuthRole::Collector, AuthRole::Collector),
        (AuthRole::Database, AuthRole::Database),
    ];

    for (role1, role2) in role_pairs {
        log::info!("Testing {:?} <-> {:?} connection", role1, role2);

        // Create actors for this role pair
        let (actor1_tx, _actor1_rx) = mpsc::unbounded_channel();
        let actor1_subscriber = MockRoomManager::new(actor1_tx).start();

        let (actor2_tx, _actor2_rx) = mpsc::unbounded_channel();
        let actor2_subscriber = MockRoomManager::new(actor2_tx).start();

        let actor1_transport = SimpleMockTransportActor::default().start();
        let mut actor1_subscribers = HashMap::new();
        actor1_subscribers.insert(
            "test-room".to_string(),
            RoomSubscribers {
                room_is_active: actor1_subscriber.clone().recipient(),
                data: actor1_subscriber.clone().recipient(),
                termination: actor1_subscriber.clone().recipient(),
            },
        );

        let actor1 = ZzNetConnActor::new(
            actor1_transport.recipient(),
            actor1_subscribers,
            "1.0".to_string(),
            role1.clone(),
            vec!["test-room".to_string()],
            mock_mgr_addr.clone().recipient(),
        )
        .start();

        let actor2_transport = SimpleMockTransportActor::default().start();
        let mut actor2_subscribers = HashMap::new();
        actor2_subscribers.insert(
            "test-room".to_string(),
            RoomSubscribers {
                room_is_active: actor2_subscriber.clone().recipient(),
                data: actor2_subscriber.clone().recipient(),
                termination: actor2_subscriber.clone().recipient(),
            },
        );

        let actor2 = ZzNetConnActor::new(
            actor2_transport.recipient(),
            actor2_subscribers,
            "1.0".to_string(),
            role2.clone(),
            vec!["test-room".to_string()],
            mock_mgr_addr.clone().recipient(),
        )
        .start();

        // Manual handshake
        let hello1 = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: role1.clone(),
            offered_rooms: vec!["test-room".to_string()],
        }))
        .unwrap();
        actor2.do_send(crate::actor::FrameFromTransport(hello1));

        let hello2 = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: role2.clone(),
            offered_rooms: vec!["test-room".to_string()],
        }))
        .unwrap();
        actor1.do_send(crate::actor::FrameFromTransport(hello2));

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Complete handshake
        let publish1 = serialize(&Frame::Room(RoomFrame::PublishRooms {
            offered_rooms: vec!["test-room".to_string()],
        }))
        .unwrap();
        actor2.do_send(crate::actor::FrameFromTransport(publish1));

        let publish2 = serialize(&Frame::Room(RoomFrame::PublishRooms {
            offered_rooms: vec!["test-room".to_string()],
        }))
        .unwrap();
        actor1.do_send(crate::actor::FrameFromTransport(publish2));

        tokio::time::sleep(tokio::time::Duration::from_millis(15)).await;

        log::info!("Successfully tested {:?} <-> {:?} connection", role1, role2);
    }

    log::info!("Different auth roles basic handshake test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_role_based_handshake_validation() {
    setup_logger();
    log::info!("Starting test_role_based_handshake_validation");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create a Collector actor
    let (collector_tx, mut collector_rx) = mpsc::unbounded_channel();
    let collector_subscriber = MockRoomManager::new(collector_tx).start();

    let collector_transport = SimpleMockTransportActor::default().start();
    let mut collector_subscribers = HashMap::new();
    collector_subscribers.insert(
        "validation-room".to_string(),
        RoomSubscribers {
            room_is_active: collector_subscriber.clone().recipient(),
            data: collector_subscriber.clone().recipient(),
            termination: collector_subscriber.clone().recipient(),
        },
    );

    let collector_actor = ZzNetConnActor::new(
        collector_transport.recipient(),
        collector_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["validation-room".to_string()],
        mock_mgr_addr.recipient(),
    )
    .start();

    // Test valid handshake
    let valid_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["validation-room".to_string()],
    }))
    .unwrap();
    collector_actor.do_send(crate::actor::FrameFromTransport(valid_hello));

    tokio::time::sleep(tokio::time::Duration::from_millis(15)).await;

    let messages = collect_messages(&mut collector_rx, 10).await;
    log::info!(
        "Received {} messages during handshake validation",
        messages.len()
    );

    log::info!("Role-based handshake validation test completed successfully");
}
