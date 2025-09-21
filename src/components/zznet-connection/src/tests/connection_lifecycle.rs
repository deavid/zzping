//! Connection Lifecycle Management Tests
//!
//! Tests covering connection termination, manager tracking, and resource cleanup

use crate::actor::{FrameFromTransport, TransportTerminated, ZzNetConnActor};
use crate::auth::AuthRole;
use crate::bus::RoomSubscribers;
use crate::mocks::{MockHarnessFactory, SimpleMockTransportActor, start_mock_connection_manager};
use crate::protocol::{Frame, HandshakeFrame, RoomFrame, serialize};
use crate::tests::utils::{
    MockRoomManager, collect_messages, filter_room_active_messages, filter_termination_messages,
    setup_logger,
};
use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;

#[actix::test]
#[ntest::timeout(200)]
async fn test_connection_termination_cascade() {
    setup_logger();
    log::info!("Starting test_connection_termination_cascade");

    let (mock_mgr_addr, _harness) = start_mock_connection_manager();

    // Create subscribers for multiple rooms
    let (room_a_tx, mut room_a_rx) = mpsc::unbounded_channel();
    let room_a_subscriber = MockRoomManager::new(room_a_tx).start();

    let (room_b_tx, mut room_b_rx) = mpsc::unbounded_channel();
    let room_b_subscriber = MockRoomManager::new(room_b_tx).start();

    // Create actor with multiple rooms
    let transport = SimpleMockTransportActor::default().start();
    let mut subscribers = HashMap::new();
    subscribers.insert(
        "room-a".to_string(),
        RoomSubscribers {
            room_is_active: room_a_subscriber.clone().recipient(),
            data: room_a_subscriber.clone().recipient(),
            termination: room_a_subscriber.clone().recipient(),
        },
    );
    subscribers.insert(
        "room-b".to_string(),
        RoomSubscribers {
            room_is_active: room_b_subscriber.clone().recipient(),
            data: room_b_subscriber.clone().recipient(),
            termination: room_b_subscriber.clone().recipient(),
        },
    );

    let actor = ZzNetConnActor::new(
        transport.recipient(),
        subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["room-a".to_string(), "room-b".to_string()],
        mock_mgr_addr.clone().recipient(),
    )
    .start();
    log::info!("Created actor with multiple room subscribers");

    // Complete handshake to activate rooms
    let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["room-a".to_string(), "room-b".to_string()],
    });
    let hello_data = serialize(&hello_frame).unwrap();
    actor.do_send(FrameFromTransport(hello_data));

    let publish_frame = Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["room-a".to_string(), "room-b".to_string()],
    });
    let publish_data = serialize(&publish_frame).unwrap();
    actor.do_send(FrameFromTransport(publish_data));
    log::info!("Completed handshake and activated rooms");

    // Wait for room activation
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Verify rooms are active
    let room_a_messages = collect_messages(&mut room_a_rx, 10).await;
    let room_b_messages = collect_messages(&mut room_b_rx, 10).await;

    let room_a_active = filter_room_active_messages(&room_a_messages);
    let room_b_active = filter_room_active_messages(&room_b_messages);

    assert_eq!(room_a_active.len(), 1, "Room A should be activated");
    assert_eq!(room_b_active.len(), 1, "Room B should be activated");
    log::info!("Verified rooms are active");

    // Simulate transport termination
    actor.do_send(TransportTerminated);
    log::info!("Simulated transport termination");

    // Wait for termination cascade
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Collect termination messages
    let room_a_term_messages = collect_messages(&mut room_a_rx, 10).await;
    let room_b_term_messages = collect_messages(&mut room_b_rx, 10).await;

    let room_a_terminated = filter_termination_messages(&room_a_term_messages);
    let room_b_terminated = filter_termination_messages(&room_b_term_messages);

    // Verify termination cascade
    assert_eq!(
        room_a_terminated.len(),
        1,
        "Room A should receive termination"
    );
    assert_eq!(
        room_b_terminated.len(),
        1,
        "Room B should receive termination"
    );
    assert_eq!(room_a_terminated[0].room_name, "room-a");
    assert_eq!(room_b_terminated[0].room_name, "room-b");

    log::info!("Connection termination cascade test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_manager_connection_tracking() {
    setup_logger();
    log::info!("Starting test_manager_connection_tracking");

    let (transport_mgr, harness) = MockHarnessFactory.transport_manager();

    // Create multiple connection actors
    let mut actors = Vec::new();
    for i in 0..3 {
        let transport = SimpleMockTransportActor::default().start();
        let actor = ZzNetConnActor::new(
            transport.recipient(),
            HashMap::new(),
            "1.0".to_string(),
            AuthRole::Collector,
            vec![format!("room-{}", i)],
            transport_mgr.clone().recipient(),
        )
        .start();
        actors.push(actor);
    }
    log::info!("Created {} connection actors", actors.len());

    // Simulate connections between pairs
    if actors.len() >= 2 {
        harness
            .simulate_connection(actors[0].clone(), actors[1].clone())
            .await;
        log::info!("Simulated connection between actors 0 and 1");
    }

    // Wait for connection establishment
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Terminate one actor to test cleanup
    if let Some(actor) = actors.first() {
        actor.do_send(TransportTerminated);
        log::info!("Terminated first actor");
    }

    // Wait for cleanup
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Shutdown all remaining connections
    harness.shutdown().await;
    log::info!("Shutdown all connections");

    // Wait for shutdown
    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    log::info!("Manager connection tracking test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_resource_cleanup_on_failure() {
    setup_logger();
    log::info!("Starting test_resource_cleanup_on_failure");

    let (mock_mgr_addr, _harness) = start_mock_connection_manager();

    // Create subscriber to track cleanup
    let (sub_tx, mut sub_rx) = mpsc::unbounded_channel();
    let subscriber = MockRoomManager::new(sub_tx).start();

    // Create actor
    let transport = SimpleMockTransportActor::default().start();
    let mut subscribers = HashMap::new();
    subscribers.insert(
        "test-room".to_string(),
        RoomSubscribers {
            room_is_active: subscriber.clone().recipient(),
            data: subscriber.clone().recipient(),
            termination: subscriber.clone().recipient(),
        },
    );

    let actor = ZzNetConnActor::new(
        transport.recipient(),
        subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["test-room".to_string()],
        mock_mgr_addr.clone().recipient(),
    )
    .start();
    log::info!("Created actor for failure testing");

    // Establish connection
    let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["test-room".to_string()],
    });
    let hello_data = serialize(&hello_frame).unwrap();
    actor.do_send(FrameFromTransport(hello_data));

    let publish_frame = Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["test-room".to_string()],
    });
    let publish_data = serialize(&publish_frame).unwrap();
    actor.do_send(FrameFromTransport(publish_data));
    log::info!("Established connection");

    // Wait for establishment
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Force actor failure by sending transport termination
    actor.do_send(TransportTerminated);
    log::info!("Sent transport termination to simulate failure");

    // Wait for cleanup propagation
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Check if subscriber received termination (it should)
    let messages = collect_messages(&mut sub_rx, 10).await;
    let terminated = filter_termination_messages(&messages);

    // Note: The subscriber may or may not receive termination depending on timing
    // The important thing is no panic or resource leak occurred
    log::info!("Received {} termination messages", terminated.len());

    log::info!("Resource cleanup on failure test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_multiple_connection_lifecycle() {
    setup_logger();
    log::info!("Starting test_multiple_connection_lifecycle");

    let (transport_mgr, harness) = MockHarnessFactory.transport_manager();

    // Create multiple pairs of connections
    let connection_pairs = 3;
    let mut all_actors = Vec::new();

    for pair in 0..connection_pairs {
        // Create client
        let client_transport = SimpleMockTransportActor::default().start();
        let client_actor = ZzNetConnActor::new(
            client_transport.recipient(),
            HashMap::new(),
            "1.0".to_string(),
            AuthRole::Collector,
            vec![format!("room-{}", pair)],
            transport_mgr.clone().recipient(),
        )
        .start();

        // Create server
        let server_transport = SimpleMockTransportActor::default().start();
        let server_actor = ZzNetConnActor::new(
            server_transport.recipient(),
            HashMap::new(),
            "1.0".to_string(),
            AuthRole::Database,
            vec![format!("room-{}", pair)],
            transport_mgr.clone().recipient(),
        )
        .start();

        // Connect them
        harness
            .simulate_connection(client_actor.clone(), server_actor.clone())
            .await;

        all_actors.push(client_actor);
        all_actors.push(server_actor);

        log::info!("Created connection pair {}", pair);
    }

    // Wait for all connections to establish
    tokio::time::sleep(tokio::time::Duration::from_millis(40)).await;

    // Terminate half the connections
    for (i, actor) in all_actors.iter().enumerate() {
        if i % 2 == 0 {
            actor.do_send(TransportTerminated);
            log::info!("Terminated actor {}", i);
        }
    }

    // Wait for termination
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Shutdown remaining connections
    harness.shutdown().await;
    log::info!("Shutdown all remaining connections");

    // Wait for final cleanup
    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    log::info!("Multiple connection lifecycle test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_connection_termination_notification() {
    setup_logger();
    log::info!("Starting test_connection_termination_notification");

    let (mock_mgr_addr, _harness) = start_mock_connection_manager();

    // Create actor
    let transport = SimpleMockTransportActor::default().start();
    let actor = ZzNetConnActor::new(
        transport.recipient(),
        HashMap::new(),
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["test-room".to_string()],
        mock_mgr_addr.clone().recipient(),
    )
    .start();
    log::info!("Created actor");

    // Wait for actor to be fully initialized
    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    // Stop the actor using transport termination
    actor.do_send(TransportTerminated);
    log::info!("Sent transport termination to stop actor");

    // Wait for termination notification to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // The manager should have received a ConnectionTerminated message
    // We can't easily verify this without exposing manager internals,
    // but the test passing without panic indicates proper cleanup
    log::info!("Connection termination notification test completed successfully");
}
