//! Data Exchange and Multiplexing Tests
//!
//! Tests covering bidirectional data flow, large frames, and multiplexing

use crate::actor::{FrameFromTransport, ZzNetConnActor};
use crate::auth::AuthRole;
use crate::bus::RoomSubscribers;
use crate::mocks::SimpleMockTransportActor;
use crate::protocol::{Frame, RoomFrame, serialize};
use crate::tests::utils::{
    MockRoomManager, collect_messages, filter_data_messages, filter_room_active_messages,
    setup_logger,
};
use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;

#[actix::test]
#[ntest::timeout(200)]
async fn test_bidirectional_data_flow() {
    setup_logger();
    log::info!("Starting test_bidirectional_data_flow");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create subscribers for both sides
    let (client_tx, mut client_rx) = mpsc::unbounded_channel();
    let client_subscriber = MockRoomManager::new(client_tx).start();

    let (server_tx, mut server_rx) = mpsc::unbounded_channel();
    let server_subscriber = MockRoomManager::new(server_tx).start();

    // Create client actor
    let client_transport = SimpleMockTransportActor::default().start();
    let mut client_subscribers = HashMap::new();
    client_subscribers.insert(
        "shared-room".to_string(),
        RoomSubscribers {
            room_is_active: client_subscriber.clone().recipient(),
            data: client_subscriber.clone().recipient(),
            termination: client_subscriber.clone().recipient(),
        },
    );

    let client_actor = ZzNetConnActor::new(
        client_transport.recipient(),
        client_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["shared-room".to_string()],
        mock_mgr_addr.clone().recipient(),
    )
    .start();
    log::info!("Created client actor");

    // Create server actor
    let server_transport = SimpleMockTransportActor::default().start();
    let mut server_subscribers = HashMap::new();
    server_subscribers.insert(
        "shared-room".to_string(),
        RoomSubscribers {
            room_is_active: server_subscriber.clone().recipient(),
            data: server_subscriber.clone().recipient(),
            termination: server_subscriber.clone().recipient(),
        },
    );

    let server_actor = ZzNetConnActor::new(
        server_transport.recipient(),
        server_subscribers,
        "1.0".to_string(),
        AuthRole::Database,
        vec!["shared-room".to_string()],
        mock_mgr_addr.recipient(),
    )
    .start();
    log::info!("Created server actor");

    // Manually send handshake frames between them
    // Client sends hello to server
    let client_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Collector,
        offered_rooms: vec!["shared-room".to_string()],
    }))
    .unwrap();
    server_actor.do_send(FrameFromTransport(client_hello));

    // Server sends hello to client
    let server_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["shared-room".to_string()],
    }))
    .unwrap();
    client_actor.do_send(FrameFromTransport(server_hello));

    // Wait for hello processing
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Client sends PublishRooms to server
    let client_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["shared-room".to_string()],
    }))
    .unwrap();
    server_actor.do_send(FrameFromTransport(client_publish));

    // Server sends PublishRooms to client
    let server_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["shared-room".to_string()],
    }))
    .unwrap();
    client_actor.do_send(FrameFromTransport(server_publish));

    // Wait for handshake completion and room activation
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Collect initial messages to verify room activation
    let client_messages = collect_messages(&mut client_rx, 10).await;
    let server_messages = collect_messages(&mut server_rx, 10).await;

    let client_active = filter_room_active_messages(&client_messages);
    let server_active = filter_room_active_messages(&server_messages);

    assert_eq!(
        client_active.len(),
        1,
        "Client should receive room activation"
    );
    assert_eq!(
        server_active.len(),
        1,
        "Server should receive room activation"
    );
    log::info!("Verified room activation on both sides");

    // Send data from client to server
    let client_data = b"Hello from client".to_vec();
    let client_message = Frame::Room(RoomFrame::MessageForRoom {
        room: "shared-room".to_string(),
        data: client_data.clone(),
    });
    let client_message_data = serialize(&client_message).unwrap();
    server_actor.do_send(FrameFromTransport(client_message_data));
    log::info!("Sent data from client to server");

    // Send data from server to client
    let server_data = b"Hello from server".to_vec();
    let server_message = Frame::Room(RoomFrame::MessageForRoom {
        room: "shared-room".to_string(),
        data: server_data.clone(),
    });
    let server_message_data = serialize(&server_message).unwrap();
    client_actor.do_send(FrameFromTransport(server_message_data));
    log::info!("Sent data from server to client");

    // Wait for data processing
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Collect data messages
    let client_data_messages = collect_messages(&mut client_rx, 10).await;
    let server_data_messages = collect_messages(&mut server_rx, 10).await;

    let client_data_msgs = filter_data_messages(&client_data_messages);
    let server_data_msgs = filter_data_messages(&server_data_messages);

    // Verify bidirectional data delivery
    assert_eq!(
        client_data_msgs.len(),
        1,
        "Client should receive server data"
    );
    assert_eq!(client_data_msgs[0].data, server_data);

    assert_eq!(
        server_data_msgs.len(),
        1,
        "Server should receive client data"
    );
    assert_eq!(server_data_msgs[0].data, client_data);

    log::info!("Bidirectional data flow test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_large_frame_handling() {
    setup_logger();
    log::info!("Starting test_large_frame_handling");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create mock subscribers
    let (client_tx, _client_rx) = mpsc::unbounded_channel();
    let client_subscriber = MockRoomManager::new(client_tx).start();

    let (server_tx, mut server_rx) = mpsc::unbounded_channel();
    let server_subscriber = MockRoomManager::new(server_tx).start();

    // Create client actor
    let client_transport = SimpleMockTransportActor::default().start();
    let mut client_subscribers = HashMap::new();
    client_subscribers.insert(
        "large-data".to_string(),
        RoomSubscribers {
            room_is_active: client_subscriber.clone().recipient(),
            data: client_subscriber.clone().recipient(),
            termination: client_subscriber.clone().recipient(),
        },
    );

    let client_actor = ZzNetConnActor::new(
        client_transport.recipient(),
        client_subscribers,
        "1.0".to_string(),
        AuthRole::Database,
        vec!["large-data".to_string()],
        mock_mgr_addr.clone().recipient(),
    )
    .start();

    // Create server actor
    let server_transport = SimpleMockTransportActor::default().start();
    let mut server_subscribers = HashMap::new();
    server_subscribers.insert(
        "large-data".to_string(),
        RoomSubscribers {
            room_is_active: server_subscriber.clone().recipient(),
            data: server_subscriber.clone().recipient(),
            termination: server_subscriber.clone().recipient(),
        },
    );

    let server_actor = ZzNetConnActor::new(
        server_transport.recipient(),
        server_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["large-data".to_string()],
        mock_mgr_addr.recipient(),
    )
    .start();

    log::info!("Established connection");

    // Manually send handshake frames
    let client_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["large-data".to_string()],
    }))
    .unwrap();
    server_actor.do_send(FrameFromTransport(client_hello));

    let server_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Collector,
        offered_rooms: vec!["large-data".to_string()],
    }))
    .unwrap();
    client_actor.do_send(FrameFromTransport(server_hello));

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // PublishRooms frames
    let client_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["large-data".to_string()],
    }))
    .unwrap();
    server_actor.do_send(FrameFromTransport(client_publish));

    let server_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["large-data".to_string()],
    }))
    .unwrap();
    client_actor.do_send(FrameFromTransport(server_publish));

    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Create large data payload (10KB)
    let large_data: Vec<u8> = (0..10240).map(|i| (i % 256) as u8).collect();
    let large_message = Frame::Room(RoomFrame::MessageForRoom {
        room: "large-data".to_string(),
        data: large_data.clone(),
    });
    let large_message_data = serialize(&large_message).unwrap();
    log::info!(
        "Created large message ({} bytes serialized)",
        large_message_data.len()
    );

    // Send large frame from client to server
    server_actor.do_send(FrameFromTransport(large_message_data));
    log::info!("Sent large frame");

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Verify large frame was handled
    let messages = collect_messages(&mut server_rx, 10).await;
    let data_messages = filter_data_messages(&messages);

    assert_eq!(data_messages.len(), 1, "Should receive large data message");
    assert_eq!(data_messages[0].data, large_data);

    log::info!("Large frame handling test completed successfully");
}

#[actix::test]
#[ntest::timeout(200)]
async fn test_high_frequency_data_exchange() {
    setup_logger();
    log::info!("Starting test_high_frequency_data_exchange");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create client subscriber
    let (client_tx, _client_rx) = mpsc::unbounded_channel();
    let client_subscriber = MockRoomManager::new(client_tx).start();

    let (server_tx, mut server_rx) = mpsc::unbounded_channel();
    let server_subscriber = MockRoomManager::new(server_tx).start();

    // Create actors
    let client_transport = SimpleMockTransportActor::default().start();
    let mut client_subscribers = HashMap::new();
    client_subscribers.insert(
        "high-freq".to_string(),
        RoomSubscribers {
            room_is_active: client_subscriber.clone().recipient(),
            data: client_subscriber.clone().recipient(),
            termination: client_subscriber.clone().recipient(),
        },
    );

    let client_actor = ZzNetConnActor::new(
        client_transport.recipient(),
        client_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["high-freq".to_string()],
        mock_mgr_addr.clone().recipient(),
    )
    .start();

    let server_transport = SimpleMockTransportActor::default().start();
    let mut server_subscribers = HashMap::new();
    server_subscribers.insert(
        "high-freq".to_string(),
        RoomSubscribers {
            room_is_active: server_subscriber.clone().recipient(),
            data: server_subscriber.clone().recipient(),
            termination: server_subscriber.clone().recipient(),
        },
    );

    let server_actor = ZzNetConnActor::new(
        server_transport.recipient(),
        server_subscribers,
        "1.0".to_string(),
        AuthRole::Database,
        vec!["high-freq".to_string()],
        mock_mgr_addr.recipient(),
    )
    .start();

    log::info!("Established connection");

    // Complete handshake
    let client_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Collector,
        offered_rooms: vec!["high-freq".to_string()],
    }))
    .unwrap();
    server_actor.do_send(FrameFromTransport(client_hello));

    let server_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["high-freq".to_string()],
    }))
    .unwrap();
    client_actor.do_send(FrameFromTransport(server_hello));

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let client_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["high-freq".to_string()],
    }))
    .unwrap();
    server_actor.do_send(FrameFromTransport(client_publish));

    let server_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["high-freq".to_string()],
    }))
    .unwrap();
    client_actor.do_send(FrameFromTransport(server_publish));

    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Send multiple messages in rapid succession
    let message_count = 10;
    for i in 0..message_count {
        let data = format!("Message {}", i).into_bytes();
        let message = Frame::Room(RoomFrame::MessageForRoom {
            room: "high-freq".to_string(),
            data,
        });
        let message_data = serialize(&message).unwrap();
        server_actor.do_send(FrameFromTransport(message_data));
    }
    log::info!("Sent {} messages in rapid succession", message_count);

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Verify all messages were received
    let messages = collect_messages(&mut server_rx, 20).await;
    let data_messages = filter_data_messages(&messages);

    assert_eq!(
        data_messages.len(),
        message_count,
        "Should receive all high-frequency messages"
    );

    log::info!("High frequency data exchange test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_concurrent_room_data_exchange() {
    setup_logger();
    log::info!("Starting test_concurrent_room_data_exchange");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create client subscriber
    let (client_tx, _client_rx) = mpsc::unbounded_channel();
    let client_subscriber = MockRoomManager::new(client_tx).start();

    let (server_tx, mut server_rx) = mpsc::unbounded_channel();
    let server_subscriber = MockRoomManager::new(server_tx).start();

    let rooms = vec![
        "room-a".to_string(),
        "room-b".to_string(),
        "room-c".to_string(),
    ];

    // Create actors with multiple rooms
    let client_transport = SimpleMockTransportActor::default().start();
    let mut client_subscribers = HashMap::new();
    for room in &rooms {
        client_subscribers.insert(
            room.clone(),
            RoomSubscribers {
                room_is_active: client_subscriber.clone().recipient(),
                data: client_subscriber.clone().recipient(),
                termination: client_subscriber.clone().recipient(),
            },
        );
    }

    let client_actor = ZzNetConnActor::new(
        client_transport.recipient(),
        client_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        rooms.clone(),
        mock_mgr_addr.clone().recipient(),
    )
    .start();

    let server_transport = SimpleMockTransportActor::default().start();
    let mut server_subscribers = HashMap::new();
    for room in &rooms {
        server_subscribers.insert(
            room.clone(),
            RoomSubscribers {
                room_is_active: server_subscriber.clone().recipient(),
                data: server_subscriber.clone().recipient(),
                termination: server_subscriber.clone().recipient(),
            },
        );
    }

    let server_actor = ZzNetConnActor::new(
        server_transport.recipient(),
        server_subscribers,
        "1.0".to_string(),
        AuthRole::Database,
        rooms.clone(),
        mock_mgr_addr.recipient(),
    )
    .start();

    log::info!("Established connection");

    // Complete handshake
    let client_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Collector,
        offered_rooms: rooms.clone(),
    }))
    .unwrap();
    server_actor.do_send(FrameFromTransport(client_hello));

    let server_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: rooms.clone(),
    }))
    .unwrap();
    client_actor.do_send(FrameFromTransport(server_hello));

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let client_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: rooms.clone(),
    }))
    .unwrap();
    server_actor.do_send(FrameFromTransport(client_publish));

    let server_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: rooms.clone(),
    }))
    .unwrap();
    client_actor.do_send(FrameFromTransport(server_publish));

    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Send data to multiple rooms concurrently
    for room in &rooms {
        let data = format!("Data for {}", room).into_bytes();
        let message = Frame::Room(RoomFrame::MessageForRoom {
            room: room.clone(),
            data,
        });
        let message_data = serialize(&message).unwrap();
        server_actor.do_send(FrameFromTransport(message_data));
        log::info!("Sent data to {}", room);
    }

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Verify data delivery to all rooms
    let messages = collect_messages(&mut server_rx, 20).await;
    let data_messages = filter_data_messages(&messages);

    assert_eq!(
        data_messages.len(),
        3,
        "Should receive data for all 3 rooms"
    );

    log::info!("Concurrent room data exchange test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_empty_and_zero_byte_messages() {
    setup_logger();
    log::info!("Starting test_empty_and_zero_byte_messages");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create mock subscribers
    let (client_tx, _client_rx) = mpsc::unbounded_channel();
    let client_subscriber = MockRoomManager::new(client_tx).start();

    let (server_tx, mut server_rx) = mpsc::unbounded_channel();
    let server_subscriber = MockRoomManager::new(server_tx).start();

    // Create actors
    let client_transport = SimpleMockTransportActor::default().start();
    let mut client_subscribers = HashMap::new();
    client_subscribers.insert(
        "empty-data".to_string(),
        RoomSubscribers {
            room_is_active: client_subscriber.clone().recipient(),
            data: client_subscriber.clone().recipient(),
            termination: client_subscriber.clone().recipient(),
        },
    );

    let client_actor = ZzNetConnActor::new(
        client_transport.recipient(),
        client_subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["empty-data".to_string()],
        mock_mgr_addr.clone().recipient(),
    )
    .start();

    let server_transport = SimpleMockTransportActor::default().start();
    let mut server_subscribers = HashMap::new();
    server_subscribers.insert(
        "empty-data".to_string(),
        RoomSubscribers {
            room_is_active: server_subscriber.clone().recipient(),
            data: server_subscriber.clone().recipient(),
            termination: server_subscriber.clone().recipient(),
        },
    );

    let server_actor = ZzNetConnActor::new(
        server_transport.recipient(),
        server_subscribers,
        "1.0".to_string(),
        AuthRole::Database,
        vec!["empty-data".to_string()],
        mock_mgr_addr.recipient(),
    )
    .start();

    log::info!("Established connection");

    // Complete handshake
    let client_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Collector,
        offered_rooms: vec!["empty-data".to_string()],
    }))
    .unwrap();
    server_actor.do_send(FrameFromTransport(client_hello));

    let server_hello = serialize(&Frame::Handshake(crate::protocol::HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["empty-data".to_string()],
    }))
    .unwrap();
    client_actor.do_send(FrameFromTransport(server_hello));

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let client_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["empty-data".to_string()],
    }))
    .unwrap();
    server_actor.do_send(FrameFromTransport(client_publish));

    let server_publish = serialize(&Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["empty-data".to_string()],
    }))
    .unwrap();
    client_actor.do_send(FrameFromTransport(server_publish));

    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Send empty message
    let empty_message = Frame::Room(RoomFrame::MessageForRoom {
        room: "empty-data".to_string(),
        data: vec![],
    });
    let empty_message_data = serialize(&empty_message).unwrap();
    server_actor.do_send(FrameFromTransport(empty_message_data));
    log::info!("Sent empty message");

    // Send zero byte message (single zero byte)
    let zero_message = Frame::Room(RoomFrame::MessageForRoom {
        room: "empty-data".to_string(),
        data: vec![0],
    });
    let zero_message_data = serialize(&zero_message).unwrap();
    server_actor.do_send(FrameFromTransport(zero_message_data));
    log::info!("Sent zero byte message");

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Verify both messages were handled
    let messages = collect_messages(&mut server_rx, 10).await;
    let data_messages = filter_data_messages(&messages);

    assert_eq!(
        data_messages.len(),
        2,
        "Should receive both empty and zero messages"
    );
    assert_eq!(
        data_messages[0].data,
        vec![],
        "First message should be empty"
    );
    assert_eq!(
        data_messages[1].data,
        vec![0],
        "Second message should contain zero byte"
    );

    log::info!("Empty and zero byte messages test completed successfully");
}
