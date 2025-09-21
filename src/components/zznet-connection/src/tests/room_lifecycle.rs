//! Room Lifecycle and Subscription Tests
//!
//! Tests covering room subscription patterns, late subscribers, and lifecycle management

use crate::actor::{FrameFromTransport, RepublishRooms, ZzNetConnActor};
use crate::auth::AuthRole;
use crate::bus::RoomSubscribers;
use crate::mocks::{start_mock_connection_manager, SimpleMockTransportActor};
use crate::protocol::{Frame, HandshakeFrame, RoomFrame, serialize};
use crate::tests::utils::{
    setup_logger, MockRoomManager, collect_messages,
    filter_room_active_messages, filter_data_messages, subscribe_mock_to_room
};
use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;

#[actix::test]
#[ntest::timeout(100)]
async fn test_late_subscriber_pattern() {
    setup_logger();
    log::info!("Starting test_late_subscriber_pattern");

    let (mock_mgr_addr, _harness) = start_mock_connection_manager();

    // Create initial subscriber
    let (early_tx, mut early_rx) = mpsc::unbounded_channel();
    let early_subscriber = MockRoomManager::new(early_tx).start();

    // Subscribe early subscriber to room-a
    subscribe_mock_to_room(&mock_mgr_addr, "room-a", &early_subscriber);
    log::info!("Early subscriber subscribed to room-a");

    // Create actor with the early subscriber
    let transport = SimpleMockTransportActor::default().start();
    let mut subscribers = HashMap::new();
    subscribers.insert(
        "room-a".to_string(),
        RoomSubscribers {
            room_is_active: early_subscriber.clone().recipient(),
            data: early_subscriber.clone().recipient(),
            termination: early_subscriber.clone().recipient(),
        },
    );

    let actor = ZzNetConnActor::new(
        transport.recipient(),
        subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["room-a".to_string()],
        mock_mgr_addr.clone().recipient(),
    ).start();
    log::info!("Created actor with early subscriber");

    // Complete handshake to activate room
    let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["room-a".to_string()],
    });
    let hello_data = serialize(&hello_frame).unwrap();
    actor.do_send(FrameFromTransport(hello_data));

    let publish_frame = Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["room-a".to_string()],
    });
    let publish_data = serialize(&publish_frame).unwrap();
    actor.do_send(FrameFromTransport(publish_data));

    log::info!("Completed handshake");

    // Wait for room activation
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Verify early subscriber got activation
    let early_messages = collect_messages(&mut early_rx, 10).await;
    let early_active = filter_room_active_messages(&early_messages);
    assert_eq!(early_active.len(), 1, "Early subscriber should receive activation");
    assert_eq!(early_active[0].room_name, "room-a");
    log::info!("Verified early subscriber received activation");

    // Now add a late subscriber
    let (late_tx, mut late_rx) = mpsc::unbounded_channel();
    let late_subscriber = MockRoomManager::new(late_tx).start();

    // Subscribe late subscriber (this should trigger republication)
    subscribe_mock_to_room(&mock_mgr_addr, "room-a", &late_subscriber);
    log::info!("Late subscriber subscribed to room-a");

    // Update actor subscribers and trigger republication
    let mut updated_subscribers = HashMap::new();
    updated_subscribers.insert(
        "room-a".to_string(),
        RoomSubscribers {
            room_is_active: late_subscriber.clone().recipient(),
            data: late_subscriber.clone().recipient(),
            termination: late_subscriber.clone().recipient(),
        },
    );

    actor.do_send(RepublishRooms {
        subscribers: updated_subscribers,
    });
    log::info!("Triggered republication for late subscriber");

    // Wait for republication
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Verify late subscriber gets activation
    let late_messages = collect_messages(&mut late_rx, 10).await;
    let late_active = filter_room_active_messages(&late_messages);
    assert_eq!(late_active.len(), 1, "Late subscriber should receive activation");
    assert_eq!(late_active[0].room_name, "room-a");

    log::info!("Late subscriber pattern test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_multiple_subscribers_per_room() {
    setup_logger();
    log::info!("Starting test_multiple_subscribers_per_room");

    let (mock_mgr_addr, _harness) = start_mock_connection_manager();

    // Create multiple subscribers for the same room
    let (sub1_tx, mut sub1_rx) = mpsc::unbounded_channel();
    let subscriber1 = MockRoomManager::new(sub1_tx).start();

    let (sub2_tx, mut sub2_rx) = mpsc::unbounded_channel();
    let subscriber2 = MockRoomManager::new(sub2_tx).start();

    let (sub3_tx, mut sub3_rx) = mpsc::unbounded_channel();
    let subscriber3 = MockRoomManager::new(sub3_tx).start();

    // Subscribe all to room-a
    subscribe_mock_to_room(&mock_mgr_addr, "room-a", &subscriber1);
    subscribe_mock_to_room(&mock_mgr_addr, "room-a", &subscriber2);
    subscribe_mock_to_room(&mock_mgr_addr, "room-a", &subscriber3);
    log::info!("Created and subscribed 3 subscribers to room-a");

    // Create actor (using subscriber1 for room subscribers for simplicity)
    let transport = SimpleMockTransportActor::default().start();
    let mut subscribers = HashMap::new();
    subscribers.insert(
        "room-a".to_string(),
        RoomSubscribers {
            room_is_active: subscriber1.clone().recipient(),
            data: subscriber1.clone().recipient(),
            termination: subscriber1.clone().recipient(),
        },
    );

    let actor = ZzNetConnActor::new(
        transport.recipient(),
        subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["room-a".to_string()],
        mock_mgr_addr.clone().recipient(),
    ).start();

    // Complete handshake
    let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["room-a".to_string()],
    });
    let hello_data = serialize(&hello_frame).unwrap();
    actor.do_send(FrameFromTransport(hello_data));

    let publish_frame = Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["room-a".to_string()],
    });
    let publish_data = serialize(&publish_frame).unwrap();
    actor.do_send(FrameFromTransport(publish_data));

    log::info!("Completed handshake");

    // Send data to the room
    let test_data = b"Test message for multiple subscribers".to_vec();
    let message_frame = Frame::Room(RoomFrame::MessageForRoom {
        room: "room-a".to_string(),
        data: test_data.clone(),
    });
    let message_data = serialize(&message_frame).unwrap();
    actor.do_send(FrameFromTransport(message_data));
    log::info!("Sent data message to room-a");

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Only subscriber1 should receive data (it's the only one actually connected to the actor)
    let sub1_messages = collect_messages(&mut sub1_rx, 10).await;
    let sub1_data = filter_data_messages(&sub1_messages);

    assert_eq!(sub1_data.len(), 1, "Subscriber1 should receive data message");
    assert_eq!(sub1_data[0].data, test_data);
    assert_eq!(sub1_data[0].room_name, "room-a");

    // Other subscribers wouldn't receive data since they're not connected to this actor
    let sub2_messages = collect_messages(&mut sub2_rx, 10).await;
    let sub3_messages = collect_messages(&mut sub3_rx, 10).await;

    // These should be empty since they're not wired to the actor
    assert_eq!(sub2_messages.len(), 0, "Subscriber2 should not receive messages from this actor");
    assert_eq!(sub3_messages.len(), 0, "Subscriber3 should not receive messages from this actor");

    log::info!("Multiple subscribers per room test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_room_republication_on_subscriber_changes() {
    setup_logger();
    log::info!("Starting test_room_republication_on_subscriber_changes");

    let (mock_mgr_addr, _harness) = start_mock_connection_manager();

    // Create initial empty actor
    let transport = SimpleMockTransportActor::default().start();
    let actor = ZzNetConnActor::new(
        transport.recipient(),
        HashMap::new(), // Start with no subscribers
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["room-a".to_string()],
        mock_mgr_addr.clone().recipient(),
    ).start();
    log::info!("Created actor with no initial subscribers");

    // Complete handshake but no rooms should activate (no subscribers)
    let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["room-a".to_string()],
    });
    let hello_data = serialize(&hello_frame).unwrap();
    actor.do_send(FrameFromTransport(hello_data));

    let publish_frame = Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["room-a".to_string()],
    });
    let publish_data = serialize(&publish_frame).unwrap();
    actor.do_send(FrameFromTransport(publish_data.clone()));
    log::info!("Completed initial handshake with no subscribers");

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    // Now add a subscriber and trigger republication
    let (sub_tx, mut sub_rx) = mpsc::unbounded_channel();
    let subscriber = MockRoomManager::new(sub_tx).start();

    let mut new_subscribers = HashMap::new();
    new_subscribers.insert(
        "room-a".to_string(),
        RoomSubscribers {
            room_is_active: subscriber.clone().recipient(),
            data: subscriber.clone().recipient(),
            termination: subscriber.clone().recipient(),
        },
    );

    // Send republish message
    actor.do_send(RepublishRooms {
        subscribers: new_subscribers,
    });
    log::info!("Triggered republication with new subscriber");

    // Resend the publish frame to simulate republication
    actor.do_send(FrameFromTransport(publish_data));

    // Wait for republication
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Verify subscriber received activation
    let messages = collect_messages(&mut sub_rx, 10).await;
    let active_messages = filter_room_active_messages(&messages);

    assert_eq!(active_messages.len(), 1, "Subscriber should receive activation after republication");
    assert_eq!(active_messages[0].room_name, "room-a");

    log::info!("Room republication on subscriber changes test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_cross_room_data_isolation() {
    setup_logger();
    log::info!("Starting test_cross_room_data_isolation");

    let (mock_mgr_addr, _harness) = start_mock_connection_manager();

    // Create subscribers for different rooms
    let (room_a_tx, mut room_a_rx) = mpsc::unbounded_channel();
    let room_a_subscriber = MockRoomManager::new(room_a_tx).start();

    let (room_b_tx, mut room_b_rx) = mpsc::unbounded_channel();
    let room_b_subscriber = MockRoomManager::new(room_b_tx).start();

    // Create actor with both rooms
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
    ).start();
    log::info!("Created actor with both room-a and room-b subscribers");

    // Complete handshake
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
    log::info!("Completed handshake with both rooms");

    // Wait for room activation
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Send data to room-a only
    let room_a_data = b"Data for room A".to_vec();
    let message_frame_a = Frame::Room(RoomFrame::MessageForRoom {
        room: "room-a".to_string(),
        data: room_a_data.clone(),
    });
    let message_data_a = serialize(&message_frame_a).unwrap();
    actor.do_send(FrameFromTransport(message_data_a));
    log::info!("Sent data to room-a");

    // Send data to room-b only
    let room_b_data = b"Data for room B".to_vec();
    let message_frame_b = Frame::Room(RoomFrame::MessageForRoom {
        room: "room-b".to_string(),
        data: room_b_data.clone(),
    });
    let message_data_b = serialize(&message_frame_b).unwrap();
    actor.do_send(FrameFromTransport(message_data_b));
    log::info!("Sent data to room-b");

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Collect messages from both subscribers
    let room_a_messages = collect_messages(&mut room_a_rx, 10).await;
    let room_b_messages = collect_messages(&mut room_b_rx, 10).await;

    // Filter data messages
    let room_a_data_msgs = filter_data_messages(&room_a_messages);
    let room_b_data_msgs = filter_data_messages(&room_b_messages);

    // Verify isolation: each room should only receive its own data
    assert_eq!(room_a_data_msgs.len(), 1, "Room A subscriber should receive one message");
    assert_eq!(room_a_data_msgs[0].room_name, "room-a");
    assert_eq!(room_a_data_msgs[0].data, room_a_data);

    assert_eq!(room_b_data_msgs.len(), 1, "Room B subscriber should receive one message");
    assert_eq!(room_b_data_msgs[0].room_name, "room-b");
    assert_eq!(room_b_data_msgs[0].data, room_b_data);

    log::info!("Cross-room data isolation test completed successfully");
}