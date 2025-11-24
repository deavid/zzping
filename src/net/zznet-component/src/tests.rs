//! Test suite for zznet-component
//!
//! Tests the "Glue Layer" that binds business logic (MainActor) to the network
//! (Router/RoomActor). This test proves that `GenericNetworkManager` and
//! `GenericRoomFactory` correctly automate the three-actor wiring ceremony.

use crate::{GenericNetworkManager, NetComponent};
use actix::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use zznet_api::{InboundRoomPayload, OnPeerConnected, PeerId, Role, RoomId, TransportFrame};
use zznet_room::{DeserializationError, RoomActor, RoomMessageTrait, SerializationError};
use zznet_router::RouterActor;

// ============================================================================
// Test Component Definition
// ============================================================================

/// Test message enum
#[derive(Serialize, Deserialize, Message, Debug, Clone, PartialEq)]
#[rtype(result = "()")]
enum TestMsg {
    Ping,
    Pong,
}

impl RoomMessageTrait for TestMsg {
    fn room_id(&self) -> RoomId {
        "test_room".into()
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        rmp_serde::to_vec(self).map_err(|e| SerializationError::MsgPackError(e.to_string()))
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        if room_id.as_str() != "test_room" {
            return Err(DeserializationError::UnknownRoom(room_id.clone()));
        }
        rmp_serde::from_slice(bytes).map_err(|e| DeserializationError::MsgPackError(e.to_string()))
    }
}

/// Test event enum
#[derive(Clone, Debug, Message)]
#[rtype(result = "()")]
enum TestEvent {
    PongEvent,
}

/// Internal message for MainActor
#[derive(Message)]
#[rtype(result = "()")]
struct InternalPing;

// ============================================================================
// MainActor (Business Logic)
// ============================================================================

/// MainActor - Simple business logic that responds to pings with pong events
struct TestMainActor {
    event_tx: tokio::sync::broadcast::Sender<TestEvent>,
}

impl TestMainActor {
    fn new(event_tx: tokio::sync::broadcast::Sender<TestEvent>) -> Self {
        Self { event_tx }
    }
}

impl Actor for TestMainActor {
    type Context = Context<Self>;
}

impl Handler<InternalPing> for TestMainActor {
    type Result = ();

    fn handle(&mut self, _msg: InternalPing, _ctx: &mut Context<Self>) -> Self::Result {
        log::debug!("MainActor received ping, sending pong event");
        let _ = self.event_tx.send(TestEvent::PongEvent);
    }
}

// ============================================================================
// NetworkActor (Translator)
// ============================================================================

/// NetworkActor - Translates between network messages and business logic
///
/// This is the canonical implementation that properly subscribes to the event bus
/// using StreamHandler. This pattern should be followed by all real components.
struct TestNetworkActor {
    _peer_id: PeerId,
    _permissions: TestPermissions,
    main_actor: Addr<TestMainActor>,
    room_actor: Addr<RoomActor<TestMsg>>,
    event_rx: tokio::sync::broadcast::Receiver<TestEvent>,
}

impl TestNetworkActor {
    fn new(
        peer_id: PeerId,
        permissions: TestPermissions,
        main_actor: Addr<TestMainActor>,
        room_actor: Addr<RoomActor<TestMsg>>,
        event_rx: tokio::sync::broadcast::Receiver<TestEvent>,
    ) -> Self {
        Self {
            _peer_id: peer_id,
            _permissions: permissions,
            main_actor,
            room_actor,
            event_rx,
        }
    }
}

impl Actor for TestNetworkActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        log::debug!("NetworkActor started");
        // Subscribe to events from event bus via StreamHandler
        let event_rx = self.event_rx.resubscribe();
        let stream = tokio_stream::wrappers::BroadcastStream::new(event_rx);
        ctx.add_stream(stream);
    }
}

// Handler for network messages (inbound from network)
impl Handler<TestMsg> for TestNetworkActor {
    type Result = ();

    fn handle(&mut self, msg: TestMsg, _ctx: &mut Context<Self>) -> Self::Result {
        log::debug!("NetworkActor received network message: {:?}", msg);
        match msg {
            TestMsg::Ping => {
                self.main_actor.do_send(InternalPing);
            }
            TestMsg::Pong => {
                log::debug!("NetworkActor received Pong");
            }
        }
    }
}

// Handler for events from MainActor via event bus (outbound to network)
impl StreamHandler<Result<TestEvent, tokio_stream::wrappers::errors::BroadcastStreamRecvError>>
    for TestNetworkActor
{
    fn handle(
        &mut self,
        msg: Result<TestEvent, tokio_stream::wrappers::errors::BroadcastStreamRecvError>,
        _ctx: &mut Context<Self>,
    ) {
        match msg {
            Ok(event) => {
                log::debug!("NetworkActor received event from stream: {:?}", event);
                match event {
                    TestEvent::PongEvent => {
                        self.room_actor.do_send(TestMsg::Pong);
                    }
                }
            }
            Err(e) => {
                log::warn!("Event stream error: {:?}", e);
            }
        }
    }
}

// ============================================================================
// Component Specification
// ============================================================================

/// Permissions for test component
#[derive(Debug, Clone, Default)]
struct TestPermissions;

/// Test component specification
struct TestComponent;

impl NetComponent for TestComponent {
    const ROOM_ID: &'static str = "test_room";
    type MainActor = TestMainActor;
    type NetworkMsg = TestMsg;
    type Event = TestEvent;
    type Permissions = TestPermissions;
    type NetworkActor = TestNetworkActor;

    fn build_network_actor(
        peer_id: PeerId,
        permissions: Self::Permissions,
        main_actor: Addr<Self::MainActor>,
        event_rx: tokio::sync::broadcast::Receiver<Self::Event>,
        room_actor: Addr<RoomActor<Self::NetworkMsg>>,
    ) -> Self::NetworkActor {
        TestNetworkActor::new(peer_id, permissions, main_actor, room_actor, event_rx)
    }
}

// ============================================================================
// The "Black Box" Test
// ============================================================================

/// Test that the GenericNetworkManager and GenericRoomFactory correctly wire
/// the three-actor pattern and enable end-to-end message flow.
///
/// This test verifies:
/// 1. Construction: GenericNetworkManager registers with Router
/// 2. Wiring: GenericRoomFactory spawns NetworkActor and wires it to RoomActor
/// 3. Communication: Messages flow from Wire -> Router -> RoomActor -> NetworkActor
///    -> MainActor -> Event -> NetworkActor -> RoomActor -> Wire
#[actix_rt::test]
async fn test_component_wiring() {
    // Initialize logging for debugging
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    log::info!("=== Phase 1: The Infrastructure ===");

    // 1. Start Router
    let router = RouterActor::new(vec![]).start();
    log::debug!("Router started");

    // 2. Create event bus
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel::<TestEvent>(100);

    // 3. Start MainActor
    let main_actor = TestMainActor::new(event_tx.clone()).start();
    log::debug!("MainActor started");

    // 4. Start GenericNetworkManager
    let permissions_map = std::collections::HashMap::new(); // Empty for this test
    let _manager = GenericNetworkManager::<TestComponent>::new(
        main_actor.clone(),
        router.clone(),
        event_tx.clone(),
        permissions_map,
    )
    .start();
    log::debug!("GenericNetworkManager started");

    // Give actors time to start and register
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    log::info!("=== Phase 2: The Connection ===");

    // 5. Create transport channels
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<TransportFrame>(10);

    // 6. Connect to Router
    let peer_id = PeerId::new("test_peer");
    let role = Role::new("test_role");
    let negotiated_rooms = vec![RoomId::from("test_room")];

    let connect_result = router
        .send(OnPeerConnected {
            peer_id: peer_id.clone(),
            role,
            negotiated_rooms,
            transport_tx: outbound_tx.clone(),
        })
        .await
        .expect("Router should respond")
        .expect("Connection should succeed");

    log::debug!(
        "Connection established, routing map: {:?}",
        connect_result.keys()
    );

    // 7. Get the recipient for our room
    let room_recipient = connect_result
        .get(&RoomId::from("test_room"))
        .expect("test_room should be in routing map")
        .clone();

    log::info!("=== Phase 3: The Stimulus (Inbound) ===");

    // 8. Create and send a Ping message
    let ping_msg = TestMsg::Ping;
    let serialized = ping_msg
        .serialize_inner()
        .expect("Serialization should succeed");
    log::debug!("Sending Ping with {} bytes", serialized.len());

    room_recipient.do_send(InboundRoomPayload {
        payload: serialized,
    });

    log::info!("=== Phase 4: The Verification (Outbound) ===");

    // 9. Wait for the Pong to arrive via transport (50ms timeout for CI safety)
    let result = tokio::time::timeout(std::time::Duration::from_millis(1), outbound_rx.recv())
        .await
        .expect("Should receive response within timeout")
        .expect("Channel should not be closed");

    log::debug!(
        "Received transport frame: {} bytes",
        result.get_bytes().len()
    );

    // 10. Deserialize and verify
    let frame =
        zznet_api::Frame::deserialize(result.get_bytes()).expect("Should deserialize as Frame");

    match frame {
        zznet_api::Frame::Room(room_frame) => match room_frame {
            zznet_api::RoomFrame::Message {
                from_room,
                to_room,
                payload,
            } => {
                log::debug!(
                    "Room message: {} -> {}, payload {} bytes",
                    from_room,
                    to_room,
                    payload.len()
                );
                assert_eq!(from_room, "test_room");
                assert_eq!(to_room, "test_room");

                // Deserialize the inner message
                let msg = TestMsg::deserialize_for_room(&RoomId::from("test_room"), &payload)
                    .expect("Should deserialize TestMsg");

                assert_eq!(
                    msg,
                    TestMsg::Pong,
                    "Should receive Pong in response to Ping"
                );
                log::info!("✓ Verification passed: Received Pong as expected");
            }
            _ => panic!("Expected Message frame, got {:?}", room_frame),
        },
        _ => panic!("Expected Room frame, got {:?}", frame),
    }

    log::info!("=== Test Complete ===");
    log::info!(
        "Logic flow verified: Inbound Ping -> RoomActor -> NetworkActor -> MainActor -> \
         Event -> NetworkActor -> RoomActor -> Outbound Pong"
    );
}

/// Simplified test that verifies just the registration and factory creation
#[actix_rt::test]
async fn test_manager_registration() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    log::info!("Testing GenericNetworkManager registration");

    // Start Router
    let router = RouterActor::new(vec![]).start();

    // Create event bus and MainActor
    let (event_tx, _) = tokio::sync::broadcast::channel::<TestEvent>(100);
    let main_actor = TestMainActor::new(event_tx.clone()).start();

    // Start Manager
    let permissions_map = std::collections::HashMap::new();
    let _manager = GenericNetworkManager::<TestComponent>::new(
        main_actor,
        router.clone(),
        event_tx,
        permissions_map,
    )
    .start();

    // Give time for registration
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // Try to connect - should get a room back
    let (outbound_tx, _) = mpsc::channel::<TransportFrame>(10);
    let result = router
        .send(OnPeerConnected {
            peer_id: PeerId::new("test"),
            role: Role::new("test"),
            negotiated_rooms: vec![RoomId::from("test_room")],
            transport_tx: outbound_tx,
        })
        .await
        .expect("Router should respond")
        .expect("Connection should succeed");

    assert!(
        result.contains_key(&RoomId::from("test_room")),
        "test_room should be registered"
    );

    log::info!("✓ Manager registration verified");
}
