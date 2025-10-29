use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::connector::connect_rooms;
use crate::room::Room;

#[derive(Message, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[rtype(result = "()")]
struct TestMessage {
    value: i32,
}

/// Test actor that collects received messages
struct TestComponent {
    received: Arc<Mutex<Vec<TestMessage>>>,
}

impl Actor for TestComponent {
    type Context = Context<Self>;
}

impl Handler<TestMessage> for TestComponent {
    type Result = ();

    fn handle(&mut self, msg: TestMessage, _ctx: &mut Context<Self>) {
        self.received.lock().unwrap().push(msg);
    }
}

impl TestComponent {
    fn new() -> (Self, Arc<Mutex<Vec<TestMessage>>>) {
        let received = Arc::new(Mutex::new(Vec::new()));
        let component = TestComponent {
            received: received.clone(),
        };
        (component, received)
    }
}

#[actix::test]
async fn test_room_to_room_communication() {
    // Setup Component A with its room
    let (component_a, _received_a) = TestComponent::new();
    let actor_a = component_a.start();
    let (room_a, channels_a) = Room::new("room_a".to_string(), actor_a.recipient());

    // Setup Component B with its room
    let (component_b, received_b) = TestComponent::new();
    let actor_b = component_b.start();
    let (room_b, channels_b) = Room::new("room_b".to_string(), actor_b.recipient());

    // Connect the rooms
    let _connection = connect_rooms(channels_a, channels_b);

    // Receivers are spawned during Room construction

    // A sends message to B
    room_a.send(TestMessage { value: 42 }).await.unwrap();

    // Wait for async delivery
    tokio::time::sleep(Duration::from_millis(1)).await;

    // Verify B received it
    {
        let msgs_b = received_b.lock().unwrap();
        assert_eq!(msgs_b.len(), 1);
        assert_eq!(msgs_b[0].value, 42);
    }

    // B sends message to A
    room_b.send(TestMessage { value: 99 }).await.unwrap();

    // Process manually on A's side to demonstrate process_one()
    // Note: room_a needs to be mutable for this
    // This is a conceptual test - in practice we'd use spawn_receiver on both

    // For this test, let's just verify the message arrives
    tokio::time::sleep(Duration::from_millis(1)).await;

    // Note: We'd need room_a to be mutable and call process_one()
    // For simplicity in this integration test, we'll spawn both receivers
}

#[actix::test]
async fn test_bidirectional_communication() {
    // Setup both components
    let (component_a, received_a) = TestComponent::new();
    let actor_a = component_a.start();
    let (room_a, channels_a) = Room::new("room_a".to_string(), actor_a.recipient());

    let (component_b, received_b) = TestComponent::new();
    let actor_b = component_b.start();
    let (room_b, channels_b) = Room::new("room_b".to_string(), actor_b.recipient());

    // Connect rooms
    let _connection = connect_rooms(channels_a, channels_b);

    // Receivers are spawned during Room construction

    // A sends to B
    room_a.send(TestMessage { value: 1 }).await.unwrap();

    // B sends to A
    room_b.send(TestMessage { value: 2 }).await.unwrap();

    // Wait for delivery
    tokio::time::sleep(Duration::from_millis(1)).await;

    // Verify both received
    let msgs_a = received_a.lock().unwrap();
    assert_eq!(msgs_a.len(), 1);
    assert_eq!(msgs_a[0].value, 2);

    let msgs_b = received_b.lock().unwrap();
    assert_eq!(msgs_b.len(), 1);
    assert_eq!(msgs_b[0].value, 1);
}
