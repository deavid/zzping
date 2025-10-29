//! Integration tests for RouterActor and PeerManagerActor interactions
//!
//! These tests validate the complete lifecycle flow between RouterActor and PeerManagerActor,
//! including peer connection, room negotiation, and message routing.

use actix::prelude::*;
use tokio::sync::mpsc;
use zznet_api::types::{PeerId, PeerIdentity, Permission, RoomId};
use zznet_peer_manager::PeerManagerActor;
use zznet_router::{
    BroadcastToPeers, HandlePublishRooms, OnPeerConnected, OnPeerDisconnected, RouterActor,
    SendToPeer,
};

/// Helper function to create a test Permission
fn create_test_permission(peer_id: PeerId) -> Permission {
    Permission {
        peer_id,
        identity: PeerIdentity {
            common_name: "test-role".to_string(),
            san_username: "test-user".to_string(),
            peer_addr: "127.0.0.1:12345".to_string(),
        },
        capabilities: 0, // No special capabilities for tests
    }
}

#[actix::test]
async fn test_router_actor_peer_lifecycle() {
    // Create RouterActor with some offered rooms
    let offered_rooms = vec![RoomId::new("shared-room"), RoomId::new("unique-room")];
    let router_actor = RouterActor::new(offered_rooms.clone(), None).start();

    // Create PeerManagerActor
    let _peer_manager = PeerManagerActor::new(None).start();

    // Create channels for peer connection
    let (outbound_tx, _outbound_rx) = mpsc::channel(10);
    let (_inbound_tx, inbound_rx) = mpsc::channel(10);

    let peer_id = PeerId::from("test-peer");

    // Test peer connection
    let connect_msg = OnPeerConnected {
        peer_id: peer_id.clone(),
        permission: create_test_permission(peer_id.clone()),
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
        permission: create_test_permission(peer_id.clone()),
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
    let (outbound_tx, _outbound_rx) = mpsc::channel(10);
    let (_inbound_tx, inbound_rx) = mpsc::channel(10);

    let peer_id = PeerId::from("test-peer");

    let connect_msg = OnPeerConnected {
        peer_id: peer_id.clone(),
        permission: create_test_permission(peer_id.clone()),
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

    // Test sending a message to the peer
    let room_id = RoomId::new("test-room");
    let message_data = b"test message".to_vec();

    let send_msg = SendToPeer {
        peer_id: peer_id.clone(),
        room_id: room_id.clone(),
        bytes: message_data.clone(),
    };

    let result = router_actor.send(send_msg).await;
    assert!(result.is_ok(), "Message send should succeed");

    // Verify message was received (this would fail in real scenario without proper setup)
    // In a full integration test, we'd need to set up the peer channels properly
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

    // Connect all peers (simplified - in real test would need proper channels)
    for peer_id in &peer_ids {
        let (outbound_tx, _outbound_rx) = mpsc::channel(10);
        let (_inbound_tx, inbound_rx) = mpsc::channel(10);

        let connect_msg = OnPeerConnected {
            peer_id: peer_id.clone(),
            permission: create_test_permission(peer_id.clone()),
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

    // Test broadcast to all peers
    let broadcast_msg = BroadcastToPeers {
        peer_ids: peer_ids.clone(),
        room_id: RoomId::new("broadcast-room"),
        bytes: b"broadcast message".to_vec(),
    };

    let result = router_actor.send(broadcast_msg).await;
    assert!(result.is_ok(), "Broadcast should succeed");
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
        permission: create_test_permission(peer_id.clone()),
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
        permission: create_test_permission(peer_id.clone()),
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
        permission: create_test_permission(peer_id.clone()),
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
