//! Integration tests for RouterActor
//!
//! These tests validate the lifecycle flow of RouterActor,
//! including peer connection, room negotiation, and message routing.

use actix::prelude::*;
use bytes::Bytes;
use tokio::sync::mpsc;
use zznet_api::error::TransportError;
use zznet_api::messages::OnPeerConnected;
use zznet_api::types::{PeerId, Role, RoomId};
use zznet_router::RouterActor;

/// Helper function to create a test Role
fn create_test_role() -> Role {
    Role::new("test-role")
}

#[actix::test]
async fn test_router_actor_peer_lifecycle() {
    // Create RouterActor with some offered rooms
    let offered_rooms = vec![RoomId::new("shared-room"), RoomId::new("unique-room")];
    let router_actor = RouterActor::new(offered_rooms.clone()).start();

    // Create transport channels for peer connection
    let (transport_tx, _transport_tx_rx) = mpsc::channel::<Bytes>(10);
    let (_transport_rx_tx, transport_rx) = mpsc::channel::<Result<Bytes, TransportError>>(10);

    let peer_id = PeerId::from("test-peer");

    // Test peer connection
    let connect_msg = OnPeerConnected {
        peer_id: peer_id.clone(),
        role: create_test_role(),
        negotiated_rooms: offered_rooms.clone(),
        transport_tx,
        transport_rx,
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
}
