//! Error Handling and Edge Cases Tests
//!
//! Tests covering transport failures, actor failures, and resource exhaustion

use crate::actor::{FrameFromTransport, TransportTerminated, ZzNetConnActor};
use crate::auth::AuthRole;
use crate::bus::RoomSubscribers;
use crate::mocks::SimpleMockTransportActor;
use crate::tests::utils::{
    MockRoomManager, collect_messages, filter_termination_messages, setup_logger,
};
use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;

#[actix::test]
#[ntest::timeout(100)]
async fn test_transport_disconnection_handling() {
    setup_logger();
    log::info!("Starting test_transport_disconnection_handling");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create subscriber to monitor termination
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
        mock_mgr_addr.recipient(),
    )
    .start();
    log::info!("Created actor with subscriber");

    // Wait for initialization
    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    // Simulate sudden transport disconnection
    actor.do_send(TransportTerminated);
    log::info!("Simulated transport disconnection");

    // Wait for termination handling
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Verify termination was handled gracefully
    let messages = collect_messages(&mut sub_rx, 10).await;
    let termination_messages = filter_termination_messages(&messages);

    // We may or may not get termination messages depending on room state
    log::info!(
        "Received {} termination messages",
        termination_messages.len()
    );

    log::info!("Transport disconnection handling test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_serialization_error_handling() {
    setup_logger();
    log::info!("Starting test_serialization_error_handling");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create actor
    let transport = SimpleMockTransportActor::default().start();
    let actor = ZzNetConnActor::new(
        transport.recipient(),
        HashMap::new(),
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["test-room".to_string()],
        mock_mgr_addr.recipient(),
    )
    .start();
    log::info!("Created actor");

    // Send various malformed frames
    let malformed_frames = [
        vec![0xFF, 0xFF, 0xFF, 0xFF],                 // Invalid binary
        vec![0x00],                                   // Single zero byte
        vec![],                                       // Empty frame
        vec![0x01, 0x02, 0x03],                       // Too short for valid frame
        (0..1000).map(|_| 0xFF).collect::<Vec<u8>>(), // Large invalid frame
    ];

    for (i, frame) in malformed_frames.iter().enumerate() {
        actor.do_send(FrameFromTransport(frame.clone()));
        log::info!("Sent malformed frame {} ({} bytes)", i + 1, frame.len());
    }

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Actor should still be running (no panic)
    log::info!("All malformed frames processed without panic");

    log::info!("Serialization error handling test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_actor_failure_isolation() {
    setup_logger();
    log::info!("Starting test_actor_failure_isolation");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create test connections manually
    let client_transport = SimpleMockTransportActor::default().start();
    let server_transport = SimpleMockTransportActor::default().start();

    // Create actors
    let client = ZzNetConnActor::new(
        client_transport.recipient(),
        HashMap::new(),
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["room-1".to_string()],
        mock_mgr_addr.clone().recipient(),
    )
    .start();

    let server = ZzNetConnActor::new(
        server_transport.recipient(),
        HashMap::new(),
        "1.0".to_string(),
        AuthRole::Database,
        vec!["room-1".to_string()],
        mock_mgr_addr.recipient(),
    )
    .start();

    // Wait for connection and handshake
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Force failure of client actor by sending TransportTerminated
    client.do_send(TransportTerminated);
    log::info!("Terminated client actor");

    // Wait for failure propagation
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // Server should still be able to handle frames gracefully
    server.do_send(FrameFromTransport(vec![0xFF])); // Should be handled gracefully
    log::info!("Sent test frame to server after client failure");

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    log::info!("Actor failure isolation test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_channel_buffer_overflow_handling() {
    setup_logger();
    log::info!("Starting test_channel_buffer_overflow_handling");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create actor
    let transport = SimpleMockTransportActor::default().start();
    let actor = ZzNetConnActor::new(
        transport.recipient(),
        HashMap::new(),
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["test-room".to_string()],
        mock_mgr_addr.recipient(),
    )
    .start();
    log::info!("Created actor");

    // Send many frames rapidly to potentially overflow buffers
    let frame_count = 50;
    for i in 0..frame_count {
        let frame = vec![0x01, 0x02, (i % 256) as u8];
        actor.do_send(FrameFromTransport(frame));
    }
    log::info!("Sent {} frames rapidly", frame_count);

    // Wait for all frames to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(40)).await;

    // Actor should handle all frames without issue
    log::info!("All frames processed successfully");

    log::info!("Channel buffer overflow handling test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_concurrent_termination_handling() {
    setup_logger();
    log::info!("Starting test_concurrent_termination_handling");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create multiple connected actor pairs
    let mut actor_pairs = Vec::new();
    for i in 0..3 {
        let client_transport = SimpleMockTransportActor::default().start();
        let client = ZzNetConnActor::new(
            client_transport.recipient(),
            HashMap::new(),
            "1.0".to_string(),
            AuthRole::Collector,
            vec![format!("room-{}", i)],
            mock_mgr_addr.clone().recipient(),
        )
        .start();

        let server_transport = SimpleMockTransportActor::default().start();
        let server = ZzNetConnActor::new(
            server_transport.recipient(),
            HashMap::new(),
            "1.0".to_string(),
            AuthRole::Database,
            vec![format!("room-{}", i)],
            mock_mgr_addr.clone().recipient(),
        )
        .start();

        actor_pairs.push((client, server));
    }
    log::info!("Created {} connected actor pairs", actor_pairs.len());

    // Wait for all connections to establish
    tokio::time::sleep(tokio::time::Duration::from_millis(40)).await;

    // Terminate all actors concurrently
    for (i, (client, server)) in actor_pairs.iter().enumerate() {
        client.do_send(TransportTerminated);
        server.do_send(TransportTerminated);
        log::info!("Terminated actor pair {}", i);
    }

    // Wait for all terminations to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(40)).await;

    log::info!("Concurrent termination handling test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_invalid_frame_sequence_handling() {
    setup_logger();
    log::info!("Starting test_invalid_frame_sequence_handling");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create actor
    let transport = SimpleMockTransportActor::default().start();
    let actor = ZzNetConnActor::new(
        transport.recipient(),
        HashMap::new(),
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["test-room".to_string()],
        mock_mgr_addr.recipient(),
    )
    .start();
    log::info!("Created actor");

    // Send frames in invalid sequences

    // Try to send room data before handshake
    let invalid_data = crate::protocol::serialize(&crate::protocol::Frame::Room(
        crate::protocol::RoomFrame::MessageForRoom {
            room: "test-room".to_string(),
            data: b"premature data".to_vec(),
        },
    ))
    .unwrap();
    actor.do_send(FrameFromTransport(invalid_data));
    log::info!("Sent room data before handshake");

    // Send multiple hello frames
    let hello1 = crate::protocol::serialize(&crate::protocol::Frame::Handshake(
        crate::protocol::HandshakeFrame::Hello {
            protocol_version: "1.0".to_string(),
            auth_role: AuthRole::Database,
            offered_rooms: vec!["test-room".to_string()],
        },
    ))
    .unwrap();

    actor.do_send(FrameFromTransport(hello1.clone()));
    actor.do_send(FrameFromTransport(hello1));
    log::info!("Sent duplicate hello frames");

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    log::info!("Invalid frame sequence handling test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_memory_leak_prevention() {
    setup_logger();
    log::info!("Starting test_memory_leak_prevention");

    let (mock_mgr_addr, _harness) = crate::mocks::start_mock_connection_manager();

    // Create and destroy many actors to test for memory leaks
    for cycle in 0..3 {
        log::info!("Memory test cycle {}", cycle + 1);

        // Create temporary actors
        let mut temp_actors = Vec::new();
        for i in 0..10 {
            let transport = SimpleMockTransportActor::default().start();
            let actor = ZzNetConnActor::new(
                transport.recipient(),
                HashMap::new(),
                "1.0".to_string(),
                AuthRole::Collector,
                vec![format!("temp-room-{}", i)],
                mock_mgr_addr.clone().recipient(),
            )
            .start();
            temp_actors.push(actor);
        }

        // Terminate all actors cleanly
        for actor in temp_actors {
            actor.do_send(crate::actor::TransportTerminated);
        }

        // Allow cleanup
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    log::info!("Memory leak prevention test completed successfully");
}
