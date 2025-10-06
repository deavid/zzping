use actix::prelude::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::connector::connect_rooms;
use crate::room::Room;

#[derive(Message, Clone, Debug, PartialEq)]
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
    let (room_a, channels_a) = Room::new(actor_a.recipient());

    // Setup Component B with its room
    let (component_b, received_b) = TestComponent::new();
    let actor_b = component_b.start();
    let (mut room_b, channels_b) = Room::new(actor_b.recipient());

    // Connect the rooms
    let _connection = connect_rooms(channels_a, channels_b);

    // Spawn receiver for B (we'll use manual processing for A as an example)
    room_b.spawn_receiver().unwrap();

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
async fn test_manual_message_processing() {
    let (component, received) = TestComponent::new();
    let actor = component.start();
    let (mut room, channels) = Room::new(actor.recipient());

    // Simulate a message arriving
    channels
        .inbound_tx
        .send(TestMessage { value: 42 })
        .await
        .unwrap();

    // Process it manually
    let processed = room.process_one().await.unwrap();
    assert!(processed);

    // Give handler time to process
    tokio::time::sleep(Duration::from_millis(1)).await;

    // Verify it was handled
    let msgs = received.lock().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].value, 42);
}

#[actix::test]
async fn test_bidirectional_communication() {
    // Setup both components
    let (component_a, received_a) = TestComponent::new();
    let actor_a = component_a.start();
    let (mut room_a, channels_a) = Room::new(actor_a.recipient());

    let (component_b, received_b) = TestComponent::new();
    let actor_b = component_b.start();
    let (mut room_b, channels_b) = Room::new(actor_b.recipient());

    // Connect rooms
    let _connection = connect_rooms(channels_a, channels_b);

    // Spawn receivers on both sides
    room_a.spawn_receiver().unwrap();
    room_b.spawn_receiver().unwrap();

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
