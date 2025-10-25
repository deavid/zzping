//! Tests for SessionManager Actor pattern
//!
//! These tests verify that SessionManager works correctly as an Actix actor,
//! with message passing instead of direct method calls.

use crate::messages::*;
use crate::peer_session::PeerSession;
use crate::session_manager::SessionManager;
use crate::types::{ConnectionState, PeerId, RoomId};
use actix::prelude::*;
use zznet_auth::mock::MockRole;

#[actix::test]
async fn test_session_manager_actor_start() {
    // Start SessionManager as an actor
    let offered_rooms = vec![RoomId::from("test-room")];
    let addr = SessionManager::<MockRole>::new(offered_rooms).start();

    // Actor should be running
    assert!(addr.connected());
}

#[actix::test]
async fn test_add_peer_via_message() {
    // Start SessionManager as an actor
    let offered_rooms = vec![RoomId::from("test-room")];
    let addr = SessionManager::<MockRole>::new(offered_rooms).start();

    // Create a peer session
    let peer_id = PeerId::from("test-peer");
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let (_tx2, rx2) = tokio::sync::mpsc::channel(10);
    let mut peer_session =
        PeerSession::<MockRole>::new_connected(peer_id.clone(), None, None, tx, rx2)
            .await
            .unwrap();
    // Start disconnected state to mimic old `PeerSession::new()`
    peer_session.disconnect();

    // Send AddPeer message
    let result = addr
        .send(AddPeer {
            peer_id: peer_id.clone(),
            peer_session,
        })
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_ok());

    // Verify peer was added by querying peer IDs
    let peer_ids = addr.send(GetPeerIds).await.unwrap();
    assert_eq!(peer_ids.len(), 1);
    assert_eq!(peer_ids[0], peer_id);
}

#[actix::test]
async fn test_get_peer_state_via_message() {
    let offered_rooms = vec![RoomId::from("test-room")];
    let addr = SessionManager::<MockRole>::new(offered_rooms).start();

    let peer_id = PeerId::from("test-peer");
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let (_tx2, rx2) = tokio::sync::mpsc::channel(10);
    let mut peer_session =
        PeerSession::<MockRole>::new_connected(peer_id.clone(), None, None, tx, rx2)
            .await
            .unwrap();
    peer_session.disconnect();

    // Add peer
    addr.send(AddPeer {
        peer_id: peer_id.clone(),
        peer_session,
    })
    .await
    .unwrap()
    .unwrap();

    // Query peer state
    let state = addr
        .send(GetPeerState {
            peer_id: peer_id.clone(),
        })
        .await
        .unwrap();

    assert_eq!(state, Some(ConnectionState::Disconnected));
}

#[actix::test]
async fn test_is_peer_connected_via_message() {
    let offered_rooms = vec![RoomId::from("test-room")];
    let addr = SessionManager::<MockRole>::new(offered_rooms).start();

    let peer_id = PeerId::from("test-peer");
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let (_tx2, rx2) = tokio::sync::mpsc::channel(10);
    let mut peer_session =
        PeerSession::<MockRole>::new_connected(peer_id.clone(), None, None, tx, rx2)
            .await
            .unwrap();
    peer_session.disconnect();

    // Add peer
    addr.send(AddPeer {
        peer_id: peer_id.clone(),
        peer_session,
    })
    .await
    .unwrap()
    .unwrap();

    // Check if connected
    let is_connected = addr
        .send(IsPeerConnected {
            peer_id: peer_id.clone(),
        })
        .await
        .unwrap();

    assert!(!is_connected); // Should be disconnected initially
}

#[actix::test]
async fn test_get_peers_with_role_via_message() {
    let offered_rooms = vec![RoomId::from("test-room")];
    let addr = SessionManager::<MockRole>::new(offered_rooms).start();

    let peer_id = PeerId::from("test-peer");
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let (_tx2, rx2) = tokio::sync::mpsc::channel(10);
    let mut peer_session = PeerSession::<MockRole>::new_connected(
        peer_id.clone(),
        Some(MockRole::Admin),
        None,
        tx,
        rx2,
    )
    .await
    .unwrap();
    peer_session.disconnect();

    // Add peer
    addr.send(AddPeer {
        peer_id: peer_id.clone(),
        peer_session,
    })
    .await
    .unwrap()
    .unwrap();

    // Query peers with Admin role
    let peers = addr
        .send(GetPeersWithRole {
            role: MockRole::Admin,
        })
        .await
        .unwrap();

    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0], peer_id);
}

#[actix::test]
async fn test_get_offered_rooms_via_message() {
    let offered_rooms = vec![RoomId::from("room1"), RoomId::from("room2")];
    let addr = SessionManager::<MockRole>::new(offered_rooms.clone()).start();

    let rooms = addr.send(GetOfferedRooms).await.unwrap();

    assert_eq!(rooms.len(), 2);
    assert!(rooms.contains(&RoomId::from("room1")));
    assert!(rooms.contains(&RoomId::from("room2")));
}

#[actix::test]
async fn test_set_offered_rooms_via_message() {
    let initial_rooms = vec![RoomId::from("room1")];
    let addr = SessionManager::<MockRole>::new(initial_rooms).start();

    // Set new rooms
    let new_rooms = vec![RoomId::from("room2"), RoomId::from("room3")];
    addr.send(SetOfferedRooms {
        rooms: new_rooms.clone(),
    })
    .await
    .unwrap();

    // Verify rooms were updated
    let rooms = addr.send(GetOfferedRooms).await.unwrap();

    assert_eq!(rooms.len(), 2);
    assert!(rooms.contains(&RoomId::from("room2")));
    assert!(rooms.contains(&RoomId::from("room3")));
}

#[actix::test]
async fn test_disconnect_peer_via_message() {
    let offered_rooms = vec![RoomId::from("test-room")];
    let addr = SessionManager::<MockRole>::new(offered_rooms).start();

    let peer_id = PeerId::from("test-peer");
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let (_tx2, rx2) = tokio::sync::mpsc::channel(10);
    let mut peer_session =
        PeerSession::<MockRole>::new_connected(peer_id.clone(), None, None, tx, rx2)
            .await
            .unwrap();
    peer_session.disconnect();

    // Add peer
    addr.send(AddPeer {
        peer_id: peer_id.clone(),
        peer_session,
    })
    .await
    .unwrap()
    .unwrap();

    // Disconnect peer
    let result = addr
        .send(DisconnectPeer {
            peer_id: peer_id.clone(),
        })
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_ok());
}

#[actix::test]
async fn test_remove_peer_via_message() {
    let offered_rooms = vec![RoomId::from("test-room")];
    let addr = SessionManager::<MockRole>::new(offered_rooms).start();

    let peer_id = PeerId::from("test-peer");
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let (_tx2, rx2) = tokio::sync::mpsc::channel(10);
    let mut peer_session =
        PeerSession::<MockRole>::new_connected(peer_id.clone(), None, None, tx, rx2)
            .await
            .unwrap();
    peer_session.disconnect();

    // Add peer
    addr.send(AddPeer {
        peer_id: peer_id.clone(),
        peer_session,
    })
    .await
    .unwrap()
    .unwrap();

    // Remove peer
    let result = addr
        .send(RemovePeer {
            peer_id: peer_id.clone(),
        })
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_ok());

    // Verify peer was removed
    let peer_ids = addr.send(GetPeerIds).await.unwrap();
    assert_eq!(peer_ids.len(), 0);
}

#[actix::test]
async fn test_get_connected_peer_count_via_message() {
    let offered_rooms = vec![RoomId::from("test-room")];
    let addr = SessionManager::<MockRole>::new(offered_rooms).start();

    // Initial count should be 0
    let count = addr.send(GetConnectedPeerCount).await.unwrap();
    assert_eq!(count, 0);

    // Add a peer (but don't connect it)
    let peer_id = PeerId::from("test-peer");
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let (_tx2, rx2) = tokio::sync::mpsc::channel(10);
    let mut peer_session =
        PeerSession::<MockRole>::new_connected(peer_id.clone(), None, None, tx, rx2)
            .await
            .unwrap();
    peer_session.disconnect();
    addr.send(AddPeer {
        peer_id,
        peer_session,
    })
    .await
    .unwrap()
    .unwrap();

    // Count should still be 0 (peer is disconnected)
    let count = addr.send(GetConnectedPeerCount).await.unwrap();
    assert_eq!(count, 0);
}

#[actix::test]
async fn test_concurrent_message_handling() {
    // Test that multiple messages can be sent concurrently
    let offered_rooms = vec![RoomId::from("test-room")];
    let addr = SessionManager::<MockRole>::new(offered_rooms).start();

    // Send multiple AddPeer messages concurrently
    let mut futures = vec![];
    for i in 0..10 {
        let peer_id = PeerId::from(format!("peer-{}", i).as_str());
        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let (_tx2, rx2) = tokio::sync::mpsc::channel(10);
        let mut peer_session =
            PeerSession::<MockRole>::new_connected(peer_id.clone(), None, None, tx, rx2)
                .await
                .unwrap();
        peer_session.disconnect();

        let fut = addr.send(AddPeer {
            peer_id,
            peer_session,
        });
        futures.push(fut);
    }

    // Wait for all to complete
    for fut in futures {
        assert!(fut.await.is_ok());
    }

    // Verify all peers were added
    let peer_ids = addr.send(GetPeerIds).await.unwrap();
    assert_eq!(peer_ids.len(), 10);
}
