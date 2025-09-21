//! Protocol State Machine Tests
//!
//! Tests covering the pure protocol logic and frame handling

use crate::actor::FrameFromTransport;
use crate::auth::AuthRole;
use crate::protocol::{Frame, Handshake, HandshakeFrame, serialize};
use crate::tests::utils::{create_test_actor, setup_logger};
use std::collections::HashMap;

#[actix::test]
#[ntest::timeout(100)]
async fn test_protocol_version_matching() {
    setup_logger();
    log::info!("Starting test_protocol_version_matching");

    // Test the handshake state machine directly
    let mut handshake = Handshake::new();

    // Create hello frame with version 1.0
    let hello_frame = handshake.create_hello_frame(
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["test-room".to_string()],
    );

    assert!(
        hello_frame.is_ok(),
        "Should successfully create hello frame"
    );
    log::info!("Successfully created hello frame with version 1.0");

    // Create another handshake for peer
    let mut peer_handshake = Handshake::new();
    let peer_hello = peer_handshake
        .create_hello_frame(
            "1.0".to_string(),
            AuthRole::Database,
            vec!["test-room".to_string()],
        )
        .unwrap();

    // Process the peer's hello frame
    let response = handshake.process_frame(peer_hello);
    assert!(
        response.is_ok(),
        "Should successfully process matching version"
    );

    log::info!("Protocol version matching test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_handshake_state_transitions() {
    setup_logger();
    log::info!("Starting test_handshake_state_transitions");

    let mut handshake = Handshake::new();

    // Initial state should be Start
    assert!(
        !handshake.is_complete(),
        "Handshake should not be complete initially"
    );

    // Create hello frame - should transition to SentHello
    let _hello_frame = handshake
        .create_hello_frame(
            "1.0".to_string(),
            AuthRole::Collector,
            vec!["test-room".to_string()],
        )
        .unwrap();

    log::info!("Created hello frame, handshake state transitioned");

    // Still not complete until peer responds
    assert!(
        !handshake.is_complete(),
        "Handshake should not be complete after sending hello"
    );

    // Create peer hello frame
    let peer_hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["test-room".to_string()],
    });
    let peer_hello_data = serialize(&peer_hello_frame).unwrap();

    // Process peer's hello
    let _response = handshake.process_frame(peer_hello_data);

    // Should now be complete
    assert!(
        handshake.is_complete(),
        "Handshake should be complete after processing peer hello"
    );

    // Should have negotiated rooms
    let active_rooms = handshake.active_rooms();
    assert!(
        active_rooms.is_some(),
        "Should have active rooms after completion"
    );
    assert_eq!(active_rooms.unwrap(), &["test-room"]);

    log::info!("Handshake state transitions test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_malformed_frame_handling() {
    setup_logger();
    log::info!("Starting test_malformed_frame_handling");

    // Create actor to test malformed frame handling
    let actor = create_test_actor(
        AuthRole::Collector,
        vec!["test-room".to_string()],
        HashMap::new(),
    );

    log::info!("Created test actor");

    // Send completely invalid data
    let invalid_data = vec![0xFF, 0xFF, 0xFF, 0xFF];
    actor.do_send(FrameFromTransport(invalid_data));
    log::info!("Sent malformed frame 1: invalid binary data");

    // Send truncated frame
    let truncated_data = vec![0x01];
    actor.do_send(FrameFromTransport(truncated_data));
    log::info!("Sent malformed frame 2: truncated data");

    // Send empty frame
    let empty_data = vec![];
    actor.do_send(FrameFromTransport(empty_data));
    log::info!("Sent malformed frame 3: empty data");

    // Wait a bit to let the actor process
    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    // Actor should still be running (graceful error handling)
    // We can't easily test this without exposing actor state, but no panic is good
    log::info!("Malformed frame handling test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_frame_ordering() {
    setup_logger();
    log::info!("Starting test_frame_ordering");

    let actor = create_test_actor(
        AuthRole::Collector,
        vec!["test-room".to_string()],
        HashMap::new(),
    );

    // Send a valid Hello frame first
    let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["test-room".to_string()],
    });
    let hello_data = serialize(&hello_frame).unwrap();
    actor.do_send(FrameFromTransport(hello_data));
    log::info!("Sent valid Hello frame");

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    // Send duplicate Hello frame (should be handled gracefully)
    let duplicate_hello = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["different-room".to_string()],
    });
    let duplicate_data = serialize(&duplicate_hello).unwrap();
    actor.do_send(FrameFromTransport(duplicate_data));
    log::info!("Sent duplicate Hello frame");

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    log::info!("Frame ordering test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_empty_room_lists() {
    setup_logger();
    log::info!("Starting test_empty_room_lists");

    let mut handshake = Handshake::new();

    // Create hello frame with empty room list
    let hello_frame = handshake.create_hello_frame(
        "1.0".to_string(),
        AuthRole::Collector,
        vec![], // Empty offered rooms
    );

    assert!(hello_frame.is_ok(), "Should handle empty room list");
    log::info!("Successfully created hello frame with empty room list");

    // Create peer with empty rooms too
    let peer_hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec![], // Empty
    });
    let peer_hello_data = serialize(&peer_hello_frame).unwrap();

    // Process peer's hello
    let _response = handshake.process_frame(peer_hello_data);

    // Should complete but with no active rooms
    assert!(
        handshake.is_complete(),
        "Handshake should complete even with no rooms"
    );

    let active_rooms = handshake.active_rooms();
    assert!(
        active_rooms.is_some(),
        "Should have room list even if empty"
    );
    assert_eq!(
        active_rooms.unwrap().len(),
        0,
        "Should have no active rooms"
    );

    log::info!("Empty room lists test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_large_room_lists() {
    setup_logger();
    log::info!("Starting test_large_room_lists");

    // Create large room lists
    let large_room_list: Vec<String> = (0..100).map(|i| format!("room-{:03}", i)).collect();

    let mut handshake = Handshake::new();

    // Create hello frame with large room list
    let hello_frame = handshake.create_hello_frame(
        "1.0".to_string(),
        AuthRole::Collector,
        large_room_list.clone(),
    );

    assert!(hello_frame.is_ok(), "Should handle large room list");
    log::info!(
        "Successfully created hello frame with {} rooms",
        large_room_list.len()
    );

    // Create peer with overlapping large list
    let peer_room_list: Vec<String> = (50..150).map(|i| format!("room-{:03}", i)).collect();

    let peer_hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: peer_room_list,
    });
    let peer_hello_data = serialize(&peer_hello_frame).unwrap();

    // Process peer's hello
    let _response = handshake.process_frame(peer_hello_data);

    // Should complete with intersection of rooms
    assert!(
        handshake.is_complete(),
        "Handshake should complete with large room lists"
    );

    let active_rooms = handshake.active_rooms();
    assert!(active_rooms.is_some(), "Should have active rooms");

    // Should have rooms 050-099 (intersection)
    let expected_count = 50;
    assert_eq!(
        active_rooms.unwrap().len(),
        expected_count,
        "Should have {} active rooms from intersection",
        expected_count
    );

    log::info!("Large room lists test completed successfully");
}

#[actix::test]
#[ntest::timeout(100)]
async fn test_special_room_names() {
    setup_logger();
    log::info!("Starting test_special_room_names");

    let special_rooms = vec![
        "room-with-dashes".to_string(),
        "room_with_underscores".to_string(),
        "room.with.dots".to_string(),
        "ROOM-WITH-CAPS".to_string(),
        "room123".to_string(),
        "room-with-very-long-name-that-might-cause-issues".to_string(),
    ];

    let mut handshake = Handshake::new();

    // Create hello frame with special room names
    let hello_frame = handshake.create_hello_frame(
        "1.0".to_string(),
        AuthRole::Collector,
        special_rooms.clone(),
    );

    assert!(hello_frame.is_ok(), "Should handle special room names");
    log::info!("Successfully created hello frame with special room names");

    // Create peer with same special rooms
    let peer_hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: special_rooms.clone(),
    });
    let peer_hello_data = serialize(&peer_hello_frame).unwrap();

    // Process peer's hello
    let _response = handshake.process_frame(peer_hello_data);

    assert!(
        handshake.is_complete(),
        "Handshake should complete with special room names"
    );

    let active_rooms = handshake.active_rooms();
    assert_eq!(
        active_rooms.unwrap().len(),
        special_rooms.len(),
        "Should activate all special rooms"
    );

    log::info!("Special room names test completed successfully");
}
