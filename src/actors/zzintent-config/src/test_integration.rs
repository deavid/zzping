#![cfg(test)]

// To run: cargo test --test integration -- --nocapture

// FIXME: This entire test file represents architectural debt - it's a proof-of-concept
// that was never completed. The following issues need to be addressed:
//
// 1. BIDIRECTIONAL COMMUNICATION MISSING:
//    - WriteData handler is a stub that doesn't actually process data
//    - Sessions should send responses/acknowledgments back to remote peers
//    - No testing of outbound intent-config protocol messages
//
// 2. PROTOCOL TRANSLATION LAYER MISSING:
//    - Test uses generic WriteData/Data messages instead of actual IntentConfigData/UpdateConfig
//    - Missing bridge between network layer and intent-config application layer
//    - No validation of actual intent-config protocol semantics
//
// 3. INCOMPLETE MOCK INFRASTRUCTURE:
//    - SimulateIncomingData exists but has no handler implementation
//    - Test bypasses proper network simulation by directly sending Data to sessions
//    - Mock transport layer incomplete (comment: "A full harness would use channels")
//
// 4. MISSING TEST SCENARIOS:
//    - No testing of configuration responses, heartbeats, or acknowledgments
//    - No error scenario testing (connection failures, malformed data)
//    - No testing of session lifecycle management
//
// TODO: Either complete the bidirectional test infrastructure or replace with focused unit tests
// TODO: Add protocol translation layer between network messages and intent-config messages
// TODO: Implement proper mock transport layer with channels for bidirectional data flow

use actix::prelude::*;
use std::collections::HashMap;

// =========================================================================
// 1. CONTRACTS (Normally in `zznet-bus` and `zznet-protocol`)
// =========================================================================

// --- RoomHandle and its Messages ---
// This is the generic handle to a single logical "Room" over the network.
// In our test, it's a handle to a MockRoom.

pub type RoomHandle = Addr<MockRoom>;

/// Message sent FROM an application TO a Room to send data.
/// FIXME: This message type exists but is never actually sent in tests
/// TODO: Implement bidirectional communication - sessions should send config responses
/// TODO: Test scenarios: acknowledgments, heartbeats, error reports, config updates
#[derive(Message, Clone)]
#[rtype(result = "()")]
#[allow(dead_code)] // Intentionally unused in this proof-of-concept - see TODO comments above
pub struct WriteData(pub Vec<u8>);

/// Message sent FROM a Room TO an application with received data.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct Data(pub Vec<u8>);

// --- RoomBus Messages ---
// These are the messages used to interact with the network bus.

/// From Application to RoomBus: "Tell me about 'room-name' rooms".
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct SubscribeToRoom {
    pub room_name: String,
    pub subscriber: Recipient<NewRoom>,
}

/// From RoomBus to RoomSupervisor: "A new room is available for you".
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct NewRoom(pub RoomHandle);

// =========================================================================
// 2. MOCK NETWORK LAYER (Test-only actors)
// =========================================================================

// --- MockRoom Actor ---
// Simulates a single, logical network channel.

/// Message our test can send TO the MockRoom to simulate receiving data.
/// FIXME: This struct is defined but never constructed - missing Handler implementation
/// TODO: Implement Handler<SimulateIncomingData> for MockRoom to enable proper network simulation
/// TODO: Use this instead of directly sending Data messages to sessions (bypasses network layer)
#[derive(Message, Clone)]
#[rtype(result = "()")]
#[allow(dead_code)] // Intentionally unused in this proof-of-concept - see TODO comments above
pub struct SimulateIncomingData(pub Vec<u8>);

pub struct MockRoom {
    // The application actor that is listening to this room.
    app_recipient: Option<Recipient<Data>>,
}

impl Actor for MockRoom {
    type Context = Context<Self>;
}

// Handler for data coming FROM the application actor.
impl Handler<WriteData> for MockRoom {
    type Result = ();
    // FIXME: Intentionally incomplete handler - represents architectural debt
    // TODO: Process msg.0 (Vec<u8>) data and forward to mock transport layer
    // TODO: Use ctx for actor lifecycle management if needed
    // TODO: Implement proper bidirectional mock transport with channels
    fn handle(&mut self, _msg: WriteData, _ctx: &mut Context<Self>) {
        println!("[MockRoom] Received data from application, forwarding to test.");
        // This is where it would send to the other side of the mock connection.
        // For this simple test, we can just log it. A full harness would use channels.
        // TODO: Replace this comment with actual implementation or remove if not needed
    }
}

// --- MockRoomBus Actor ---
// Simulates the NetworkBoundary. Its job is to create MockRooms.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SimulateConnection;

pub struct MockRoomBus {
    subscribers: HashMap<String, Recipient<NewRoom>>,
}

impl MockRoomBus {
    fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
        }
    }
}

impl Actor for MockRoomBus {
    type Context = Context<Self>;
}

impl Handler<SubscribeToRoom> for MockRoomBus {
    type Result = ();
    fn handle(&mut self, msg: SubscribeToRoom, _ctx: &mut Context<Self>) {
        println!(
            "[MockRoomBus] Got a new subscriber for room '{}'",
            msg.room_name
        );
        self.subscribers.insert(msg.room_name, msg.subscriber);
    }
}

// This is where our test triggers a "connection".
impl Handler<SimulateConnection> for MockRoomBus {
    type Result = ();
    fn handle(&mut self, _msg: SimulateConnection, _ctx: &mut Context<Self>) {
        if let Some(subscriber) = self.subscribers.get("intent-config") {
            println!("[MockRoomBus] Simulating new room for 'intent-config'");

            let mock_room = MockRoom {
                app_recipient: None,
            }
            .start();

            subscriber.do_send(NewRoom(mock_room));
        }
    }
}

// =========================================================================
// 3. GENERIC REUSABLE ACTOR (Normally in `zznet-bus`)
// =========================================================================

/// Message FROM the RoomSupervisor TO the AppSupervisor.
#[derive(Message)]
#[rtype(result = "()")]
pub struct CreateSession(pub RoomHandle);

/// The generic RoomSupervisor. Spawns session actors.
pub struct RoomSupervisor {
    app_supervisor: Recipient<CreateSession>,
}
impl RoomSupervisor {
    pub fn new(app_supervisor: Recipient<CreateSession>) -> Self {
        Self { app_supervisor }
    }
}
impl Actor for RoomSupervisor {
    type Context = Context<Self>;
}

// Its only job is to receive a new Room and tell the AppSupervisor to create a session.
impl Handler<NewRoom> for RoomSupervisor {
    type Result = ();
    fn handle(&mut self, msg: NewRoom, _ctx: &mut Context<Self>) {
        println!("[RoomSupervisor] Received a new room, telling AppSupervisor to create session.");
        self.app_supervisor.do_send(CreateSession(msg.0));
    }
}

// =========================================================================
// 4. APPLICATION ACTORS (Normally in `zzintent-config`)
// =========================================================================

// --- App Supervisor ---
#[derive(Default)]
pub struct IntentConfigSupervisorActor {
    state: String,
    sessions: Vec<Addr<IntentConfigSessionActor>>,
}
impl IntentConfigSupervisorActor {
    fn new() -> Self {
        Self::default()
    }
}
impl Actor for IntentConfigSupervisorActor {
    type Context = Context<Self>;
}

// Message to get the internal state for assertions.
#[derive(Message)]
#[rtype(result = "String")]
pub struct GetState;
impl Handler<GetState> for IntentConfigSupervisorActor {
    type Result = String;
    fn handle(&mut self, _msg: GetState, _ctx: &mut Context<Self>) -> Self::Result {
        self.state.clone()
    }
}

// It knows how to create its own session workers.
impl Handler<CreateSession> for IntentConfigSupervisorActor {
    type Result = ();
    fn handle(&mut self, msg: CreateSession, ctx: &mut Context<Self>) {
        println!("[AppSupervisor] Creating and starting a new session actor.");
        let session = IntentConfigSessionActor::new(msg.0, ctx.address().recipient()).start();
        self.sessions.push(session);
    }
}

// It handles internal state updates from its children.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct InternalUpdate(pub String);
impl Handler<InternalUpdate> for IntentConfigSupervisorActor {
    type Result = ();
    fn handle(&mut self, msg: InternalUpdate, _ctx: &mut Context<Self>) {
        println!(
            "[AppSupervisor] Received internal state update from session: '{}'",
            msg.0
        );
        self.state = msg.0;
    }
}

// --- App Session Actor ---
pub struct IntentConfigSessionActor {
    room: RoomHandle,
    supervisor: Recipient<InternalUpdate>,
}
impl IntentConfigSessionActor {
    pub fn new(room: RoomHandle, supervisor: Recipient<InternalUpdate>) -> Self {
        Self { room, supervisor }
    }
}
impl Actor for IntentConfigSessionActor {
    type Context = Context<Self>;
    fn started(&mut self, ctx: &mut Context<Self>) {
        // Tell the room that WE are the ones who want to receive data.
        let self_recipient = ctx.address().recipient();
        self.room.do_send(SetRecipient(self_recipient));
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct SetRecipient(pub Recipient<Data>);
impl Handler<SetRecipient> for MockRoom {
    type Result = ();
    fn handle(&mut self, msg: SetRecipient, _ctx: &mut Context<Self>) {
        self.app_recipient = Some(msg.0);
    }
}

// Handler for data coming from the MockRoom.
impl Handler<Data> for IntentConfigSessionActor {
    type Result = ();
    fn handle(&mut self, msg: Data, _ctx: &mut Context<Self>) {
        let text = String::from_utf8_lossy(&msg.0);
        println!("[AppSession] Received data from room: '{}'", text);
        // TODO: This is using a simple string protocol instead of actual intent-config messages
        // TODO: Replace with proper deserialization of IntentConfigData, UpdateConfig, etc.
        // TODO: Add error handling for malformed protocol messages
        // TODO: Implement protocol versioning and backwards compatibility
        if let Some(update) = text.strip_prefix("UPDATE: ") {
            // TODO: Convert string to proper IntentConfigData structure
            // TODO: Send UpdateConfig message to actual IntentConfigActor instead of simple string
            self.supervisor.do_send(InternalUpdate(update.to_string()));
        }
        // TODO: Add support for other message types: Subscribe, Unsubscribe, config requests
        // TODO: Send acknowledgments back via self.room.do_send(WriteData(...))
    }
}

// =========================================================================
// 5. THE TEST
// =========================================================================

#[actix::test]
async fn test_full_lifecycle_proof_of_concept() {
    // NOTE: This is an intentionally incomplete proof-of-concept test
    // See file-level FIXME comments for comprehensive list of architectural debt
    // Current limitations:
    // - Only tests unidirectional communication (incoming data)
    // - Uses string-based protocol instead of actual IntentConfigData messages
    // - Bypasses network layer simulation (sends Data directly to sessions)
    // - No testing of outbound messages, error scenarios, or protocol edge cases
    use std::time::Duration;
    let _ = env_logger::builder().is_test(true).try_init();

    // ARRANGE: Start all the actors
    let bus = MockRoomBus::new().start();
    let app_supervisor = IntentConfigSupervisorActor::new().start();
    let room_supervisor = RoomSupervisor::new(app_supervisor.clone().recipient()).start();

    // ARRANGE: Wire them up by subscribing.
    bus.do_send(SubscribeToRoom {
        room_name: "intent-config".to_string(),
        subscriber: room_supervisor.recipient(),
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // ACT: Simulate a client connecting.
    println!("\n--- TEST: Simulating a new connection ---");
    bus.do_send(SimulateConnection);
    tokio::time::sleep(Duration::from_millis(50)).await;

    // ACT: Simulate the remote peer sending an "Update" message.
    println!("\n--- TEST: Simulating incoming data ---");
    // FIXME: This comment reveals the architectural problems in the test design:
    // "This part is tricky with the MockRoom setup, a full harness would be better"
    // TODO: Implement proper SimulateIncomingData handler instead of bypassing network layer
    // (This part is tricky with the MockRoom setup, a full harness would be better)
    // A real test harness would hold the other side of the mpsc channels.
    // For this PoC, we assert on the state change.

    // ASSERT: Check that the Application Supervisor's state was NOT updated yet.
    let state = app_supervisor.send(GetState).await.unwrap();
    assert_eq!(state, ""); // Default state

    // FIXME: This bypasses the intended network simulation architecture
    // TODO: Replace with proper network-layer simulation: bus.do_send(SimulateIncomingData(...))
    // TODO: Add protocol translation: convert raw bytes to IntentConfigData messages
    // Simulate sending data to the session actor (a real test would do this via the mock bus)
    let sessions = app_supervisor.send(GetSessions).await.unwrap();
    assert_eq!(sessions.len(), 1);
    sessions[0].do_send(Data(b"UPDATE: new_data".to_vec()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    // ASSERT: Check that the Application Supervisor's state was updated.
    let state = app_supervisor.send(GetState).await.unwrap();
    assert_eq!(state, "new_data");

    println!("\n--- TEST: Proof of concept successful ---");
}

#[derive(Message)]
#[rtype(result = "Vec<Addr<IntentConfigSessionActor>>")]
struct GetSessions;

impl Handler<GetSessions> for IntentConfigSupervisorActor {
    type Result = Vec<Addr<IntentConfigSessionActor>>;
    fn handle(&mut self, _msg: GetSessions, _ctx: &mut Self::Context) -> Self::Result {
        self.sessions.clone()
    }
}
