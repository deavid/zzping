//! Integration tests for RouterActor
//!
//! These tests validate the lifecycle flow of RouterActor,
//! including peer connection, room negotiation, and message routing.

use actix::prelude::*;
use tokio::sync::mpsc;
use zznet_api::types::{PeerId, Role, RoomId};
use zznet_router::{
    HandlePublishRooms, OnPeerConnected, OnPeerDisconnected, PeerSender, RouterActor,
};

/// Helper function to create a test Role
fn create_test_role() -> Role {
    Role::new("test-role")
}

#[actix::test]
async fn test_router_actor_peer_lifecycle() {
    // Create RouterActor with some offered rooms
    let offered_rooms = vec![RoomId::new("shared-room"), RoomId::new("unique-room")];
    let router_actor = RouterActor::new(offered_rooms.clone(), None).start();

    // Create channels for peer connection
    let (outbound_tx, _outbound_rx) = mpsc::channel(10);
    let (_inbound_tx, inbound_rx) = mpsc::channel(10);

    let peer_id = PeerId::from("test-peer");

    // Test peer connection
    let connect_msg = OnPeerConnected {
        peer_id: peer_id.clone(),
        role: create_test_role(),
        outbound_tx,
        inbound_rx,
    };

    let result = router_actor.send(connect_msg).await;
    assert!(
        result.is_ok(),
        "Peer connection message should be sent successfully"
    );

    let connect_result = result.unwrap();
    assert!(
        connect_result.is_ok(),
        "Peer connection should succeed: {:?}",
        connect_result
    );

    // Test peer disconnection
    let disconnect_msg = OnPeerDisconnected {
        peer_id: peer_id.clone(),
    };

    let result = router_actor.send(disconnect_msg).await;
    assert!(result.is_ok(), "Peer disconnection should succeed");
}

#[actix::test]
async fn test_router_actor_room_negotiation() {
    // Create RouterActor with offered rooms
    let offered_rooms = vec![RoomId::new("shared-room"), RoomId::new("unique-room")];
    let router_actor = RouterActor::new(offered_rooms.clone(), None).start();

    let peer_id = PeerId::from("test-peer");
    let peer_rooms = vec![RoomId::new("shared-room"), RoomId::new("peer-only-room")];

    // First connect the peer
    let (outbound_tx, _outbound_rx) = mpsc::channel(10);
    let (_inbound_tx, inbound_rx) = mpsc::channel(10);

    let connect_msg = OnPeerConnected {
        peer_id: peer_id.clone(),
        role: create_test_role(),
        outbound_tx,
        inbound_rx,
    };

    let connect_result = router_actor.send(connect_msg).await.unwrap();
    assert!(connect_result.is_ok(), "Peer connection should succeed");

    // Test room negotiation
    let negotiate_msg = HandlePublishRooms {
        peer_id: peer_id.clone(),
        peer_rooms: peer_rooms.clone(),
    };

    let result = router_actor.send(negotiate_msg).await;
    assert!(
        result.is_ok(),
        "Room negotiation message should be sent successfully"
    );

    let inner_result = result.unwrap();
    assert!(inner_result.is_ok(), "Room negotiation should succeed");

    let joined_rooms = inner_result.unwrap();
    // Should only include the intersection: "shared-room"
    assert_eq!(joined_rooms.len(), 1, "Should have exactly one joined room");
    assert_eq!(
        joined_rooms[0],
        RoomId::new("shared-room"),
        "Should join shared room"
    );
}

#[actix::test]
async fn test_router_actor_message_routing() {
    // Create RouterActor
    let offered_rooms = vec![RoomId::new("test-room")];
    let router_actor = RouterActor::new(offered_rooms, None).start();

    // Create a peer and connect it
    let (outbound_tx, mut outbound_rx) = mpsc::channel(10);
    let (_inbound_tx, inbound_rx) = mpsc::channel(10);

    let peer_id = PeerId::from("test-peer");

    let connect_msg = OnPeerConnected {
        peer_id: peer_id.clone(),
        role: create_test_role(),
        outbound_tx,
        inbound_rx,
    };

    let result = router_actor.send(connect_msg).await.unwrap();
    assert!(result.is_ok(), "Peer connection should succeed");

    // Negotiate rooms to establish connection
    let negotiate_msg = HandlePublishRooms {
        peer_id: peer_id.clone(),
        peer_rooms: vec![RoomId::new("test-room")], // Use the same room as offered
    };
    let negotiate_result = router_actor.send(negotiate_msg).await.unwrap();
    assert!(negotiate_result.is_ok(), "Room negotiation should succeed");

    // Get the peer's outbound sender from the RouterActor
    let peer_sender_msg = zznet_router::PeerSender {
        peer_id: peer_id.clone(),
    };

    let peer_sender_result = router_actor.send(peer_sender_msg).await.unwrap();
    assert!(peer_sender_result.is_some(), "Should get peer sender");

    let peer_sender = peer_sender_result.unwrap();

    // Test sending a message to the peer using the direct channel
    let room_id = RoomId::new("test-room");
    let message_data = b"test message".to_vec();

    let send_result = peer_sender
        .send((room_id.clone(), message_data.clone()))
        .await;
    assert!(send_result.is_ok(), "Direct message send should succeed");

    // Verify message was received on the outbound channel
    let received = outbound_rx.recv().await;
    assert!(
        received.is_some(),
        "Should receive message on outbound channel"
    );

    let (received_room_id, received_data) = received.unwrap();
    assert_eq!(received_room_id, room_id, "Room ID should match");
    assert_eq!(received_data, message_data, "Message data should match");
}

#[actix::test]
async fn test_router_actor_broadcast() {
    // Create RouterActor
    let offered_rooms = vec![RoomId::new("broadcast-room")];
    let router_actor = RouterActor::new(offered_rooms, None).start();

    // Create multiple peers
    let peer_ids = vec![
        PeerId::from("peer-1"),
        PeerId::from("peer-2"),
        PeerId::from("peer-3"),
    ];

    // Connect all peers (keep outbound receivers alive)
    let mut outbound_receivers = Vec::new();
    for peer_id in &peer_ids {
        let (outbound_tx, outbound_rx) = mpsc::channel(10);
        let (_inbound_tx, inbound_rx) = mpsc::channel(10);
        outbound_receivers.push(outbound_rx);

        let connect_msg = OnPeerConnected {
            peer_id: peer_id.clone(),
            role: create_test_role(),
            outbound_tx,
            inbound_rx,
        };

        let connect_result = router_actor.send(connect_msg).await.unwrap();
        assert!(connect_result.is_ok(), "Peer connection should succeed");

        // Negotiate rooms
        let negotiate_msg = HandlePublishRooms {
            peer_id: peer_id.clone(),
            peer_rooms: vec![RoomId::new("broadcast-room")],
        };
        let negotiate_result = router_actor.send(negotiate_msg).await.unwrap();
        assert!(negotiate_result.is_ok(), "Room negotiation should succeed");
    }

    // Test broadcast to all peers by getting each peer sender and sending individually
    let room_id = RoomId::new("broadcast-room");
    let message_bytes = b"broadcast message".to_vec();

    for peer_id in &peer_ids {
        let peer_sender_msg = PeerSender {
            peer_id: peer_id.clone(),
        };
        let peer_sender_option = router_actor.send(peer_sender_msg).await.unwrap();
        assert!(peer_sender_option.is_some(), "Should get peer sender");

        let peer_sender = peer_sender_option.unwrap();
        let send_result = peer_sender
            .send((room_id.clone(), message_bytes.clone()))
            .await;
        assert!(send_result.is_ok(), "Broadcast message send should succeed");
    }
}

#[actix::test]
async fn test_router_actor_edge_cases() {
    let router_actor = RouterActor::new(vec![], None).start();

    // Test connecting same peer twice
    let peer_id = PeerId::from("duplicate-peer");
    let (outbound_tx1, _outbound_rx1) = mpsc::channel(10);
    let (_inbound_tx1, inbound_rx1) = mpsc::channel(10);

    let connect_msg1 = OnPeerConnected {
        peer_id: peer_id.clone(),
        role: create_test_role(),
        outbound_tx: outbound_tx1,
        inbound_rx: inbound_rx1,
    };

    let connect_result1 = router_actor.send(connect_msg1).await.unwrap();
    assert!(
        connect_result1.is_ok(),
        "First peer connection should succeed"
    );

    // Second connection should handle gracefully (depending on implementation)
    let (outbound_tx2, _outbound_rx2) = mpsc::channel(10);
    let (_inbound_tx2, inbound_rx2) = mpsc::channel(10);

    let connect_msg2 = OnPeerConnected {
        peer_id: peer_id.clone(),
        role: create_test_role(),
        outbound_tx: outbound_tx2,
        inbound_rx: inbound_rx2,
    };

    // This should either succeed or fail gracefully depending on implementation
    let result = router_actor.send(connect_msg2).await;
    // We don't assert here as the behavior depends on the implementation details
    let _ = result;

    // Test disconnecting non-existent peer
    let disconnect_msg = OnPeerDisconnected {
        peer_id: PeerId::from("non-existent-peer"),
    };

    let result = router_actor.send(disconnect_msg).await;
    assert!(
        result.is_ok(),
        "Disconnecting non-existent peer should not fail"
    );
}

#[actix::test]
async fn test_router_actor_empty_room_intersection() {
    let offered_rooms = vec![RoomId::new("router-room")];
    let router_actor = RouterActor::new(offered_rooms, None).start();

    // Connect the peer first
    let (outbound_tx, _outbound_rx) = mpsc::channel(10);
    let (_inbound_tx, inbound_rx) = mpsc::channel(10);

    let peer_id = PeerId::from("test-peer");

    let connect_msg = OnPeerConnected {
        peer_id: peer_id.clone(),
        role: create_test_role(),
        outbound_tx,
        inbound_rx,
    };

    let connect_result = router_actor.send(connect_msg).await.unwrap();
    assert!(connect_result.is_ok(), "Peer connection should succeed");

    let peer_rooms = vec![RoomId::new("peer-room")]; // No overlap

    let negotiate_msg = HandlePublishRooms {
        peer_id: peer_id.clone(),
        peer_rooms,
    };

    let result = router_actor.send(negotiate_msg).await;
    assert!(
        result.is_ok(),
        "Room negotiation message should be sent successfully"
    );

    let inner_result = result.unwrap();
    assert!(
        inner_result.is_err(),
        "Room negotiation should fail with empty intersection"
    );

    let error = inner_result.unwrap_err();
    assert!(
        error.contains("EmptyIntersection"),
        "Should return EmptyIntersection error: {}",
        error
    );
}
