//! Test suite for zznet-router
//!
//! Tests the Control Plane of the Router, verifying that it correctly coordinates
//! RoomFactory instances and returns proper routing maps to the HelloActor.

use crate::{RegisterManager, RoomFactory, RoomFactoryRef, RouterActor};
use actix::prelude::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use zznet_api::{
    InboundRoomPayload, OnPeerConnected, PeerId, Role, RoomId, RoomInboundRecipient, TransportFrame,
};

// ============================================================================
// Mock Objects
// ============================================================================

/// MockEndpoint - Acts as a Room destination
///
/// Receives payloads and pipes them into a test channel for verification.
struct MockEndpoint {
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl MockEndpoint {
    fn new(tx: mpsc::UnboundedSender<Vec<u8>>) -> Self {
        Self { tx }
    }
}

impl Actor for MockEndpoint {
    type Context = Context<Self>;
}

impl Handler<InboundRoomPayload> for MockEndpoint {
    type Result = ();

    fn handle(&mut self, msg: InboundRoomPayload, _ctx: &mut Context<Self>) -> Self::Result {
        let _ = self.tx.send(msg.payload);
    }
}

/// MockFactory - Creates room actors
///
/// Returns a pre-configured recipient when asked to create a room.
struct MockFactory {
    recipient: RoomInboundRecipient,
}

impl MockFactory {
    fn new(recipient: RoomInboundRecipient) -> Self {
        Self { recipient }
    }
}

impl RoomFactory for MockFactory {
    fn create_room(
        &self,
        _peer_id: PeerId,
        _role: Role,
        _room_id: RoomId,
        _transport_tx: mpsc::Sender<TransportFrame>,
    ) -> Result<Option<RoomInboundRecipient>, String> {
        Ok(Some(self.recipient.clone()))
    }
}

// ============================================================================
// The "Sorting Hat" Test
// ============================================================================

/// Test that the Router correctly delegates room creation to registered factories
/// and returns a proper routing map.
///
/// This test verifies:
/// 1. Factory registration works correctly
/// 2. OnPeerConnected triggers room creation via correct factories
/// 3. Returned routing map contains correct recipients
/// 4. Messages route to the correct destination (isolation)
#[actix_rt::test]
async fn test_sorting_hat() {
    // Phase 1: The Stage Setup
    // -------------------------

    // Create channels for receiving routed messages
    let (tx_a, mut rx_a) = mpsc::unbounded_channel::<Vec<u8>>();
    let (tx_b, mut rx_b) = mpsc::unbounded_channel::<Vec<u8>>();

    // Spawn mock endpoints (representing Room A and Room B)
    let endpoint_a = MockEndpoint::new(tx_a).start();
    let recipient_a = endpoint_a.recipient::<InboundRoomPayload>();

    let endpoint_b = MockEndpoint::new(tx_b).start();
    let recipient_b = endpoint_b.recipient::<InboundRoomPayload>();

    // Create factories that will produce these endpoints
    let factory_a: RoomFactoryRef = Arc::new(MockFactory::new(recipient_a));
    let factory_b: RoomFactoryRef = Arc::new(MockFactory::new(recipient_b));

    // Start the Router
    let router = RouterActor::new(vec![RoomId::from("Room_A"), RoomId::from("Room_B")]).start();

    // Register factories with their respective rooms
    router
        .send(RegisterManager {
            factory: factory_a,
            rooms: vec![RoomId::from("Room_A")],
        })
        .await
        .expect("Failed to send RegisterManager A")
        .expect("Failed to register factory A");

    router
        .send(RegisterManager {
            factory: factory_b,
            rooms: vec![RoomId::from("Room_B")],
        })
        .await
        .expect("Failed to send RegisterManager B")
        .expect("Failed to register factory B");

    // Phase 2: The Connection Simulation
    // -----------------------------------

    // Create a dummy transport channel (required by OnPeerConnected but not used in this test)
    let (transport_tx, _transport_rx) = mpsc::channel::<TransportFrame>(1);

    // Simulate peer connection with negotiated rooms
    let routing_map = router
        .send(OnPeerConnected {
            peer_id: PeerId::from("test_peer"),
            role: Role::from("test_role"),
            negotiated_rooms: vec![RoomId::from("Room_A"), RoomId::from("Room_B")],
            transport_tx,
        })
        .await
        .expect("Failed to send OnPeerConnected")
        .expect("OnPeerConnected returned error");

    // Phase 3: The Verification (Routing & Isolation)
    // ------------------------------------------------

    // Verify the routing map contains both rooms
    let room_a = RoomId::from("Room_A");
    let room_b = RoomId::from("Room_B");

    assert!(
        routing_map.contains_key(&room_a),
        "Routing map missing Room_A"
    );
    assert!(
        routing_map.contains_key(&room_b),
        "Routing map missing Room_B"
    );
    assert_eq!(
        routing_map.len(),
        2,
        "Routing map should contain exactly 2 rooms"
    );

    // Extract recipients from the routing map
    let recipient_for_a = routing_map
        .get(&room_a)
        .expect("Room_A not in routing map")
        .clone();
    let recipient_for_b = routing_map
        .get(&room_b)
        .expect("Room_B not in routing map")
        .clone();

    // Route messages through the recipients
    recipient_for_a
        .send(InboundRoomPayload {
            payload: b"Magic A".to_vec(),
        })
        .await
        .expect("Failed to send to Room_A recipient");

    recipient_for_b
        .send(InboundRoomPayload {
            payload: b"Magic B".to_vec(),
        })
        .await
        .expect("Failed to send to Room_B recipient");

    // Verify messages arrived at the correct destinations
    let received_a = rx_a.recv().await.expect("No message received on channel A");
    assert_eq!(received_a, b"Magic A", "Room A received wrong payload");

    let received_b = rx_b.recv().await.expect("No message received on channel B");
    assert_eq!(received_b, b"Magic B", "Room B received wrong payload");

    // Verify isolation: channels should not have cross-contamination
    // (Since we only sent one message to each, trying to receive more should timeout or be empty)
    // We'll just verify we got the right messages above, which confirms the Router
    // correctly mapped Room IDs to the correct Factory-created recipients.
}
