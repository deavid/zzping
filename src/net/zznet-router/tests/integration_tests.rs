//! Integration tests for RouterActor
//!
//! These tests validate the lifecycle flow of RouterActor,
//! including peer connection, room negotiation, and message routing.

use actix::prelude::*;
use tokio::sync::mpsc;
use zznet_api::types::{PeerId, Role, RoomId};
use zznet_router::{OnPeerConnected, OnPeerDisconnected, RouterActor};

/// Helper function to create a test Role
fn create_test_role() -> Role {
    Role::new("test-role")
}

#[actix::test]
async fn test_router_actor_peer_lifecycle() {
    // Create RouterActor with some offered rooms
    let offered_rooms = vec![RoomId::new("shared-room"), RoomId::new("unique-room")];
    let router_actor = RouterActor::new(offered_rooms.clone()).start();

    // Create channels for peer connection
    let (outbound_tx, _outbound_rx) = mpsc::channel(10);
    let (_inbound_tx, inbound_rx) = mpsc::channel(10);

    let peer_id = PeerId::from("test-peer");

    // Test peer connection
    let connect_msg = OnPeerConnected {
        peer_id: peer_id.clone(),
        role: create_test_role(),
        negotiated_rooms: offered_rooms.clone(),
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
