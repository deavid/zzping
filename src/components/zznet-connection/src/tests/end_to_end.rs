//! End-to-End Connection Tests
//!
//! Tests covering complete handshake protocols between ZzNetConnActor instances

use crate::actor::ZzNetConnActor;
use crate::auth::AuthRole;
use crate::bus::RoomSubscribers;
use crate::mocks::SimpleMockTransportActor;
use crate::tests::utils::{
    MockRoomManager, collect_messages, filter_room_active_messages, setup_logger,
};
use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;

#[actix::test]
#[ntest::timeout(100)]
async fn test_basic_symmetric_handshake() {
    setup_logger();
    log::info!("Starting test_basic_symmetric_handshake");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create mock room managers for both sides
    let (client_tx, mut client_rx) = mpsc::unbounded_channel();
    let client_room_mgr = MockRoomManager::new(client_tx).start();

    let (server_tx, mut server_rx) = mpsc::unbounded_channel();
    let server_room_mgr = MockRoomManager::new(server_tx).start();

    // Create client side subscribers
    let mut client_subscribers = HashMap::new();
    client_subscribers.insert(
        "intent-config".to_string(),
        RoomSubscribers {
            room_is_active: client_room_mgr.clone().recipient(),
            data: client_room_mgr.clone().recipient(),
            termination: client_room_mgr.clone().recipient(),
        },
    );

    // Create server side subscribers
    let mut server_subscribers = HashMap::new();
    server_subscribers.insert(
        "intent-config".to_string(),
        RoomSubscribers {
            room_is_active: server_room_mgr.clone().recipient(),
            data: server_room_mgr.clone().recipient(),
            termination: server_room_mgr.clone().recipient(),
        },
    );

    // Create client actor
    let client_transport = SimpleMockTransportActor::default().start();
    let client_actor = ZzNetConnActor::new(
        client_transport.recipient(),
        client_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["intent-config".to_string()],
        mock_mgr_addr.clone().recipient(),
    )
    .start();
    log::info!("Created client actor");

    // Create server actor
    let server_transport = SimpleMockTransportActor::default().start();
    let server_actor = ZzNetConnActor::new(
        server_transport.recipient(),
        server_subscribers,
        "1.0".to_string(),
        AuthRole::Database,
        vec!["intent-config".to_string()],
        mock_mgr_addr.recipient(),
    )
    .start();
    log::info!("Created server actor");

    // Manually send handshake frames between them
    // Client sends hello to server
    let client_hello = crate::protocol::serialize(&crate::protocol::Frame::Handshake(
        crate::protocol::HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: AuthRole::Collector,
            offered_rooms: vec!["intent-config".to_string()],
        },
    ))
    .unwrap();

    server_actor.do_send(crate::actor::FrameFromTransport(client_hello));

    // Server sends hello to client
    let server_hello = crate::protocol::serialize(&crate::protocol::Frame::Handshake(
        crate::protocol::HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: AuthRole::Database,
            offered_rooms: vec!["intent-config".to_string()],
        },
    ))
    .unwrap();

    client_actor.do_send(crate::actor::FrameFromTransport(server_hello));

    // Wait for hello processing
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Client sends PublishRooms to server
    let client_publish = crate::protocol::serialize(&crate::protocol::Frame::Room(
        crate::protocol::RoomFrame::PublishRooms {
            offered_rooms: vec!["intent-config".to_string()],
        },
    ))
    .unwrap();

    server_actor.do_send(crate::actor::FrameFromTransport(client_publish));

    // Server sends PublishRooms to client
    let server_publish = crate::protocol::serialize(&crate::protocol::Frame::Room(
        crate::protocol::RoomFrame::PublishRooms {
            offered_rooms: vec!["intent-config".to_string()],
        },
    ))
    .unwrap();

    client_actor.do_send(crate::actor::FrameFromTransport(server_publish));

    // Wait for handshake completion and room activation
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Collect messages from both sides
    let client_messages = collect_messages(&mut client_rx, 20).await;
    let server_messages = collect_messages(&mut server_rx, 20).await;

    log::info!("Client received {} messages", client_messages.len());
    log::info!("Server received {} messages", server_messages.len());

    // Verify both sides received room activation
    let client_active = filter_room_active_messages(&client_messages);
    let server_active = filter_room_active_messages(&server_messages);

    assert_eq!(client_active.len(), 1, "Client should receive room activation");
    assert_eq!(server_active.len(), 1, "Server should receive room activation");

    log::info!("Basic symmetric handshake test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_asymmetric_room_offerings() {
    setup_logger();
    log::info!("Starting test_asymmetric_room_offerings");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create mock room managers
    let (client_tx, mut client_rx) = mpsc::unbounded_channel();
    let client_room_mgr = MockRoomManager::new(client_tx).start();

    let (server_tx, mut server_rx) = mpsc::unbounded_channel();
    let server_room_mgr = MockRoomManager::new(server_tx).start();

    // Client offers room-a and room-b
    let mut client_subscribers = HashMap::new();
    client_subscribers.insert(
        "room-a".to_string(),
        RoomSubscribers {
            room_is_active: client_room_mgr.clone().recipient(),
            data: client_room_mgr.clone().recipient(),
            termination: client_room_mgr.clone().recipient(),
        },
    );
    client_subscribers.insert(
        "room-b".to_string(),
        RoomSubscribers {
            room_is_active: client_room_mgr.clone().recipient(),
            data: client_room_mgr.clone().recipient(),
            termination: client_room_mgr.clone().recipient(),
        },
    );

    // Server offers room-b and room-c
    let mut server_subscribers = HashMap::new();
    server_subscribers.insert(
        "room-b".to_string(),
        RoomSubscribers {
            room_is_active: server_room_mgr.clone().recipient(),
            data: server_room_mgr.clone().recipient(),
            termination: server_room_mgr.clone().recipient(),
        },
    );
    server_subscribers.insert(
        "room-c".to_string(),
        RoomSubscribers {
            room_is_active: server_room_mgr.clone().recipient(),
            data: server_room_mgr.clone().recipient(),
            termination: server_room_mgr.clone().recipient(),
        },
    );

    // Create actors with different room sets
    let client_transport = SimpleMockTransportActor::default().start();
    let client_actor = ZzNetConnActor::new(
        client_transport.recipient(),
        client_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["room-a".to_string(), "room-b".to_string()],
        mock_mgr_addr.clone().recipient(),
    ).start();
    log::info!("Created client actor");

    let server_transport = SimpleMockTransportActor::default().start();
    let server_actor = ZzNetConnActor::new(
        server_transport.recipient(),
        server_subscribers,
        "1.0".to_string(),
        AuthRole::Database,
        vec!["room-b".to_string(), "room-c".to_string()],
        mock_mgr_addr.recipient(),
    ).start();
    log::info!("Created server actor");

    log::info!("Created actors with asymmetric room offerings");

    // Manual handshake
    let client_hello = crate::protocol::serialize(&crate::protocol::Frame::Handshake(
        crate::protocol::HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: AuthRole::Collector,
            offered_rooms: vec!["room-a".to_string(), "room-b".to_string()],
        },
    )).unwrap();
    server_actor.do_send(crate::actor::FrameFromTransport(client_hello));

    let server_hello = crate::protocol::serialize(&crate::protocol::Frame::Handshake(
        crate::protocol::HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: AuthRole::Database,
            offered_rooms: vec!["room-b".to_string(), "room-c".to_string()],
        },
    )).unwrap();
    client_actor.do_send(crate::actor::FrameFromTransport(server_hello));

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Complete handshake with PublishRooms
    let client_publish = crate::protocol::serialize(&crate::protocol::Frame::Room(
        crate::protocol::RoomFrame::PublishRooms {
            offered_rooms: vec!["room-a".to_string(), "room-b".to_string()],
        },
    )).unwrap();
    server_actor.do_send(crate::actor::FrameFromTransport(client_publish));

    let server_publish = crate::protocol::serialize(&crate::protocol::Frame::Room(
        crate::protocol::RoomFrame::PublishRooms {
            offered_rooms: vec!["room-b".to_string(), "room-c".to_string()],
        },
    )).unwrap();
    client_actor.do_send(crate::actor::FrameFromTransport(server_publish));

    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    // Collect messages to verify room activation
    let client_messages = collect_messages(&mut client_rx, 10).await;
    let server_messages = collect_messages(&mut server_rx, 10).await;

    // Verify only room-b becomes active (intersection)
    let client_active = filter_room_active_messages(&client_messages);
    let server_active = filter_room_active_messages(&server_messages);

    assert_eq!(client_active.len(), 1, "Client should receive one room activation");
    assert_eq!(server_active.len(), 1, "Server should receive one room activation");
    assert_eq!(client_active[0].room_name, "room-b", "Active room should be room-b");
    assert_eq!(server_active[0].room_name, "room-b", "Active room should be room-b");

    log::info!("Asymmetric room offerings test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_no_common_rooms() {
    setup_logger();
    log::info!("Starting test_no_common_rooms");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create mock room managers
    let (client_tx, mut client_rx) = mpsc::unbounded_channel();
    let client_room_mgr = MockRoomManager::new(client_tx).start();

    let (server_tx, mut server_rx) = mpsc::unbounded_channel();
    let server_room_mgr = MockRoomManager::new(server_tx).start();

    // Client offers only room-a
    let mut client_subscribers = HashMap::new();
    client_subscribers.insert(
        "room-a".to_string(),
        RoomSubscribers {
            room_is_active: client_room_mgr.clone().recipient(),
            data: client_room_mgr.clone().recipient(),
            termination: client_room_mgr.clone().recipient(),
        },
    );

    // Server offers only room-b
    let mut server_subscribers = HashMap::new();
    server_subscribers.insert(
        "room-b".to_string(),
        RoomSubscribers {
            room_is_active: server_room_mgr.clone().recipient(),
            data: server_room_mgr.clone().recipient(),
            termination: server_room_mgr.clone().recipient(),
        },
    );

    // Create actors with completely different rooms
    let client_transport = SimpleMockTransportActor::default().start();
    let client_actor = ZzNetConnActor::new(
        client_transport.recipient(),
        client_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["room-a".to_string()],
        mock_mgr_addr.clone().recipient(),
    ).start();

    let server_transport = SimpleMockTransportActor::default().start();
    let server_actor = ZzNetConnActor::new(
        server_transport.recipient(),
        server_subscribers,
        "1.0".to_string(),
        AuthRole::Database,
        vec!["room-b".to_string()],
        mock_mgr_addr.recipient(),
    ).start();

    log::info!("Created actors with no common rooms");

    // Manual handshake
    let client_hello = crate::protocol::serialize(&crate::protocol::Frame::Handshake(
        crate::protocol::HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: AuthRole::Collector,
            offered_rooms: vec!["room-a".to_string()],
        },
    )).unwrap();
    server_actor.do_send(crate::actor::FrameFromTransport(client_hello));

    let server_hello = crate::protocol::serialize(&crate::protocol::Frame::Handshake(
        crate::protocol::HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: AuthRole::Database,
            offered_rooms: vec!["room-b".to_string()],
        },
    )).unwrap();
    client_actor.do_send(crate::actor::FrameFromTransport(server_hello));

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Complete handshake with PublishRooms
    let client_publish = crate::protocol::serialize(&crate::protocol::Frame::Room(
        crate::protocol::RoomFrame::PublishRooms {
            offered_rooms: vec!["room-a".to_string()],
        },
    )).unwrap();
    server_actor.do_send(crate::actor::FrameFromTransport(client_publish));

    let server_publish = crate::protocol::serialize(&crate::protocol::Frame::Room(
        crate::protocol::RoomFrame::PublishRooms {
            offered_rooms: vec!["room-b".to_string()],
        },
    )).unwrap();
    client_actor.do_send(crate::actor::FrameFromTransport(server_publish));

    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    // Collect messages to verify no room activation
    let client_messages = collect_messages(&mut client_rx, 10).await;
    let server_messages = collect_messages(&mut server_rx, 10).await;

    // Verify no rooms become active (empty intersection)
    let client_active = filter_room_active_messages(&client_messages);
    let server_active = filter_room_active_messages(&server_messages);

    assert_eq!(client_active.len(), 0, "Client should receive no room activations");
    assert_eq!(server_active.len(), 0, "Server should receive no room activations");

    log::info!("No common rooms test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_multiple_rooms_partial_overlap() {
    setup_logger();
    log::info!("Starting test_multiple_rooms_partial_overlap");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create mock room managers
    let (client_tx, mut client_rx) = mpsc::unbounded_channel();
    let client_room_mgr = MockRoomManager::new(client_tx).start();

    let (server_tx, mut server_rx) = mpsc::unbounded_channel();
    let server_room_mgr = MockRoomManager::new(server_tx).start();

    // Client offers room-a, room-b, room-c
    let client_rooms = vec!["room-a".to_string(), "room-b".to_string(), "room-c".to_string()];
    let mut client_subscribers = HashMap::new();
    for room in &client_rooms {
        client_subscribers.insert(
            room.clone(),
            RoomSubscribers {
                room_is_active: client_room_mgr.clone().recipient(),
                data: client_room_mgr.clone().recipient(),
                termination: client_room_mgr.clone().recipient(),
            },
        );
    }

    // Server offers room-b, room-c, room-d
    let server_rooms = vec!["room-b".to_string(), "room-c".to_string(), "room-d".to_string()];
    let mut server_subscribers = HashMap::new();
    for room in &server_rooms {
        server_subscribers.insert(
            room.clone(),
            RoomSubscribers {
                room_is_active: server_room_mgr.clone().recipient(),
                data: server_room_mgr.clone().recipient(),
                termination: server_room_mgr.clone().recipient(),
            },
        );
    }

    // Create actors with partial room overlap
    let client_transport = SimpleMockTransportActor::default().start();
    let client_actor = ZzNetConnActor::new(
        client_transport.recipient(),
        client_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        client_rooms.clone(),
        mock_mgr_addr.clone().recipient(),
    ).start();

    let server_transport = SimpleMockTransportActor::default().start();
    let server_actor = ZzNetConnActor::new(
        server_transport.recipient(),
        server_subscribers,
        "1.0".to_string(),
        AuthRole::Database,
        server_rooms.clone(),
        mock_mgr_addr.recipient(),
    ).start();

    log::info!("Created actors with partial room overlap");

    // Manual handshake
    let client_hello = crate::protocol::serialize(&crate::protocol::Frame::Handshake(
        crate::protocol::HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: AuthRole::Collector,
            offered_rooms: client_rooms.clone(),
        },
    )).unwrap();
    server_actor.do_send(crate::actor::FrameFromTransport(client_hello));

    let server_hello = crate::protocol::serialize(&crate::protocol::Frame::Handshake(
        crate::protocol::HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: AuthRole::Database,
            offered_rooms: server_rooms.clone(),
        },
    )).unwrap();
    client_actor.do_send(crate::actor::FrameFromTransport(server_hello));

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Complete handshake with PublishRooms
    let client_publish = crate::protocol::serialize(&crate::protocol::Frame::Room(
        crate::protocol::RoomFrame::PublishRooms {
            offered_rooms: client_rooms,
        },
    )).unwrap();
    server_actor.do_send(crate::actor::FrameFromTransport(client_publish));

    let server_publish = crate::protocol::serialize(&crate::protocol::Frame::Room(
        crate::protocol::RoomFrame::PublishRooms {
            offered_rooms: server_rooms,
        },
    )).unwrap();
    client_actor.do_send(crate::actor::FrameFromTransport(server_publish));

    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    // Collect messages to verify room activation
    let client_messages = collect_messages(&mut client_rx, 10).await;
    let server_messages = collect_messages(&mut server_rx, 10).await;

    // Verify room-b and room-c become active (intersection)
    let client_active = filter_room_active_messages(&client_messages);
    let server_active = filter_room_active_messages(&server_messages);

    assert_eq!(client_active.len(), 2, "Client should receive two room activations");
    assert_eq!(server_active.len(), 2, "Server should receive two room activations");

    // Verify the correct rooms are active
    let mut client_active_rooms: Vec<String> = client_active.iter().map(|m| m.room_name.clone()).collect();
    let mut server_active_rooms: Vec<String> = server_active.iter().map(|m| m.room_name.clone()).collect();
    client_active_rooms.sort();
    server_active_rooms.sort();

    assert_eq!(client_active_rooms, vec!["room-b", "room-c"]);
    assert_eq!(server_active_rooms, vec!["room-b", "room-c"]);

    log::info!("Multiple rooms partial overlap test completed successfully");
}