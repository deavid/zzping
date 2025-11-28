//! Negotiation test for HelloActor handshake logic.
//!
//! This test proves that the `HelloActor` correctly negotiates a set of active rooms
//! by intersecting local offerings with remote offerings during the Session Layer handshake.

use crate::actor::HelloConfig;
use crate::session_messages::HandshakeComplete;
use actix::prelude::*;
use std::time::Duration;
use tokio::sync::mpsc;
use zznet_api::{Frame, HandshakeFrame, TransportFrame, create_mock_pair};

/// Mock SessionManager Actor that receives HandshakeComplete messages
struct MockSessionManager {
    handshake_received: mpsc::UnboundedSender<HandshakeComplete>,
}

impl Actor for MockSessionManager {
    type Context = Context<Self>;
}

impl Handler<HandshakeComplete> for MockSessionManager {
    type Result = ();

    fn handle(&mut self, msg: HandshakeComplete, _ctx: &mut Context<Self>) {
        let _ = self.handshake_received.send(msg);
    }
}

#[actix::test]
async fn test_negotiation_intersection() {
    // ═════════════════════════════════════════════════════════════════════════
    // A. Setup Phase
    // ═════════════════════════════════════════════════════════════════════════

    // 1. Channel: Create the mpsc channel for the result
    let (tx, mut handshake_rx) = mpsc::unbounded_channel();

    // 2. Observer: Spawn MockSessionManager
    let session_manager = MockSessionManager {
        handshake_received: tx,
    }
    .start();
    let session_recipient = session_manager.recipient();

    // 3. The Wire: Create mock pair
    let (local_transport, mut remote_transport) = create_mock_pair("test_nego");

    // 4. The Subject: Create HelloConfig for the server
    let config = HelloConfig {
        our_role: "server".to_string(),
        offered_rooms: vec!["Room_A".to_string(), "Room_B".to_string()],
        handshake_timeout: Duration::from_secs(10),
        hostname: "server-host".to_string(),
    };

    // 5. Execution: Spawn the actor
    let local_conn = local_transport.into_established();
    let _hello_actor = crate::actor::start_hello_actor_with_handshake_recipient(
        local_conn.tx,
        local_conn.rx,
        "server".to_string(),
        None,
        config,
        Some(session_recipient),
    );

    // ═════════════════════════════════════════════════════════════════════════
    // B. The Interaction (The Handshake)
    // ═════════════════════════════════════════════════════════════════════════

    // 1. Read Server Hello
    let server_hello_frame_data = remote_transport
        .recv()
        .await
        .expect("Failed to receive server HELLO frame");
    let server_hello_frame = Frame::deserialize(server_hello_frame_data.get_bytes())
        .expect("Failed to deserialize server HELLO");

    // Assert it is Hello
    match server_hello_frame {
        Frame::Handshake(HandshakeFrame::Hello {
            version,
            role_str,
            hostname,
        }) => {
            assert_eq!(version, "1.0");
            assert_eq!(role_str, "server");
            assert_eq!(hostname, "server-host");
        }
        _ => panic!(
            "Expected HandshakeFrame::Hello, got {:?}",
            server_hello_frame
        ),
    }

    // 2. Read Server Offer
    let server_offer_frame_data = remote_transport
        .recv()
        .await
        .expect("Failed to receive server OFFER frame");
    let server_offer_frame = Frame::deserialize(server_offer_frame_data.get_bytes())
        .expect("Failed to deserialize server OFFER");

    // Assert it is Offer with Room_A and Room_B
    match server_offer_frame {
        Frame::Handshake(HandshakeFrame::Offer { rooms }) => {
            assert_eq!(rooms.len(), 2);
            assert!(rooms.contains(&"Room_A".to_string()));
            assert!(rooms.contains(&"Room_B".to_string()));
        }
        _ => panic!(
            "Expected HandshakeFrame::Offer, got {:?}",
            server_offer_frame
        ),
    }

    // 3. Send Client Hello
    let client_hello = Frame::Handshake(HandshakeFrame::Hello {
        version: "1.0".to_string(),
        role_str: "client".to_string(),
        hostname: "client-host".to_string(),
    });
    let client_hello_data = client_hello
        .serialize()
        .expect("Failed to serialize client HELLO");
    remote_transport
        .send(TransportFrame::new(client_hello_data))
        .await
        .expect("Failed to send client HELLO");

    // 4. Send Client Offer
    // Client offers Room_B and Room_C
    // Intersection should only be Room_B
    let client_offer = Frame::Handshake(HandshakeFrame::Offer {
        rooms: vec!["Room_B".to_string(), "Room_C".to_string()],
    });
    let client_offer_data = client_offer
        .serialize()
        .expect("Failed to serialize client OFFER");
    remote_transport
        .send(TransportFrame::new(client_offer_data))
        .await
        .expect("Failed to send client OFFER");

    // ═════════════════════════════════════════════════════════════════════════
    // C. Verification Phase
    // ═════════════════════════════════════════════════════════════════════════

    // 1. Await Result
    let result: HandshakeComplete = handshake_rx.recv().await.expect("HandshakeComplete was not received");

    // 2. Assert Intersection
    assert_eq!(
        result.active_rooms.len(),
        1,
        "Expected 1 active room, got {:?}",
        result.active_rooms
    );
    assert!(
        result.active_rooms.contains(&"Room_B".to_string()),
        "Room_B must be in active_rooms"
    );
    assert!(
        !result.active_rooms.contains(&"Room_A".to_string()),
        "Room_A must NOT be in active_rooms (client didn't offer it)"
    );
    assert!(
        !result.active_rooms.contains(&"Room_C".to_string()),
        "Room_C must NOT be in active_rooms (server didn't offer it)"
    );

    // Verify peer information
    assert_eq!(result.peer_id, "client-host");
    assert_eq!(result.peer_role_str, "client");
}
