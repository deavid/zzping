use crate::actor::{FrameFromTransport, TransportTerminated, ZzNetConnActor};
use crate::auth::AuthRole;
use crate::bus::{DataForRoom, RoomIsActive, RoomTerminated};
use crate::mocks::{start_mock_connection_manager, SimpleMockTransportActor};
use crate::protocol::{Frame, HandshakeFrame, RoomFrame, serialize};
use actix::prelude::*;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
enum TestMessage {
    RoomIsActive(RoomIsActive),
    DataForRoom(DataForRoom),
    RoomTerminated(RoomTerminated),
}

#[derive(Default)]
struct MockRoomManager {
    sender: Option<mpsc::UnboundedSender<TestMessage>>,
}

impl MockRoomManager {
    fn new(sender: mpsc::UnboundedSender<TestMessage>) -> Self {
        Self {
            sender: Some(sender),
        }
    }
}

impl Actor for MockRoomManager {
    type Context = Context<Self>;
}

impl Handler<RoomIsActive> for MockRoomManager {
    type Result = ();

    fn handle(&mut self, msg: RoomIsActive, _ctx: &mut Context<Self>) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(TestMessage::RoomIsActive(msg));
        }
    }
}

impl Handler<DataForRoom> for MockRoomManager {
    type Result = ();

    fn handle(&mut self, msg: DataForRoom, _ctx: &mut Context<Self>) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(TestMessage::DataForRoom(msg));
        }
    }
}

impl Handler<RoomTerminated> for MockRoomManager {
    type Result = ();

    fn handle(&mut self, msg: RoomTerminated, _ctx: &mut Context<Self>) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(TestMessage::RoomTerminated(msg));
        }
    }
}

#[actix::test]
#[ntest::timeout(1000)]
async fn test_full_handshake_and_notification() {
    let (mock_mgr_addr, _harness) = start_mock_connection_manager();
    let room_mgr = MockRoomManager::default().start();

    // Subscribe to "intent-config"
    mock_mgr_addr.do_send(crate::bus::SubscribeToRoom {
        room_name: "intent-config".to_string(),
        room_is_active_recipient: room_mgr.clone().recipient(),
        data_recipient: room_mgr.clone().recipient(), // Mock doesn't handle, but for test
        termination_recipient: room_mgr.clone().recipient(), // Mock doesn't handle
    });

    // Start the actor manually
    let transport = SimpleMockTransportActor::default().start();
    let mut subscribers = std::collections::HashMap::new();
    subscribers.insert(
        "intent-config".to_string(),
        crate::bus::RoomSubscribers {
            room_is_active: room_mgr.clone().recipient::<RoomIsActive>(),
            data: room_mgr.clone().recipient(), // Not used in this test
            termination: room_mgr.clone().recipient(), // Not used
        },
    );
    let actor = ZzNetConnActor::new(
        transport.recipient(),
        subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["intent-config".to_string()],
        mock_mgr_addr.clone().recipient(),
    );
    let actor_addr = actor.start();

    // Send Hello from peer
    let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["intent-config".to_string()],
    });
    let hello_data = serialize(&hello_frame).unwrap();
    actor_addr.do_send(FrameFromTransport(hello_data));

    // Send PublishRooms from peer
    let publish_frame = Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["intent-config".to_string()],
    });
    let publish_data = serialize(&publish_frame).unwrap();
    actor_addr.do_send(FrameFromTransport(publish_data));

    // Wait a bit
    tokio::time::sleep(Duration::from_millis(100)).await;

    // For now, just ensure no panic
}

#[actix::test]
#[ntest::timeout(1000)]
async fn test_late_subscriber() {
    let (mock_mgr_addr, _harness) = start_mock_connection_manager();

    // Start actor first
    let transport = SimpleMockTransportActor::default().start();
    let subscribers = std::collections::HashMap::new();
    let actor = ZzNetConnActor::new(
        transport.recipient(),
        subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["intent-config".to_string()],
        mock_mgr_addr.clone().recipient(),
    );
    let actor_addr = actor.start();

    // Complete handshake
    let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["intent-config".to_string()],
    });
    let hello_data = serialize(&hello_frame).unwrap();
    actor_addr.do_send(FrameFromTransport(hello_data));

    let publish_frame = Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["intent-config".to_string()],
    });
    let publish_data = serialize(&publish_frame).unwrap();
    actor_addr.do_send(FrameFromTransport(publish_data.clone()));

    // Wait
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Now subscribe
    let room_mgr = MockRoomManager::default().start();
    mock_mgr_addr.clone().do_send(crate::bus::SubscribeToRoom {
        room_name: "intent-config".to_string(),
        room_is_active_recipient: room_mgr.clone().recipient(),
        data_recipient: room_mgr.clone().recipient(),
        termination_recipient: room_mgr.clone().recipient(),
    });

    // Wait
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send PublishRooms again from peer
    actor_addr.do_send(FrameFromTransport(publish_data));

    // Wait
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Check
}

#[actix::test]
#[ntest::timeout(1000)]
async fn test_data_round_trip() {
    let (mock_mgr_addr, _harness) = start_mock_connection_manager();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let room_mgr = MockRoomManager::new(tx).start();

    // Subscribe to "room-a"
    mock_mgr_addr.do_send(crate::bus::SubscribeToRoom {
        room_name: "room-a".to_string(),
        room_is_active_recipient: room_mgr.clone().recipient(),
        data_recipient: room_mgr.clone().recipient(),
        termination_recipient: room_mgr.clone().recipient(),
    });

    // Start the actor
    let transport = SimpleMockTransportActor::default().start();
    let mut subscribers = std::collections::HashMap::new();
    subscribers.insert(
        "room-a".to_string(),
        crate::bus::RoomSubscribers {
            room_is_active: room_mgr.clone().recipient::<RoomIsActive>(),
            data: room_mgr.clone().recipient::<DataForRoom>(),
            termination: room_mgr.clone().recipient::<RoomTerminated>(),
        },
    );
    let actor = ZzNetConnActor::new(
        transport.recipient(),
        subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["room-a".to_string()],
        mock_mgr_addr.clone().recipient(),
    );
    let actor_addr = actor.start();

    // Send Hello from peer
    let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["room-a".to_string()],
    });
    let hello_data = serialize(&hello_frame).unwrap();
    actor_addr.do_send(FrameFromTransport(hello_data));

    // Send PublishRooms from peer
    let publish_frame = Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["room-a".to_string()],
    });
    let publish_data = serialize(&publish_frame).unwrap();
    actor_addr.do_send(FrameFromTransport(publish_data));

    // Wait for handshake to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Collect received messages
    let mut received_room_is_active = Vec::new();
    let mut received_data = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        match msg {
            TestMessage::RoomIsActive(m) => received_room_is_active.push(m),
            TestMessage::DataForRoom(m) => received_data.push(m),
            TestMessage::RoomTerminated(_) => {}
        }
    }

    // Assert subscriber received RoomIsActive
    assert_eq!(received_room_is_active.len(), 1);
    assert_eq!(received_room_is_active[0].room_name, "room-a");

    // Now test data reception: send a MessageForRoom frame to the actor
    let test_data = b"Hello from peer!".to_vec();
    let message_frame = Frame::Room(RoomFrame::MessageForRoom {
        room: "room-a".to_string(),
        data: test_data.clone(),
    });
    let message_data = serialize(&message_frame).unwrap();
    actor_addr.do_send(FrameFromTransport(message_data));

    // Wait
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Collect again
    while let Ok(msg) = rx.try_recv() {
        match msg {
            TestMessage::RoomIsActive(_) => {}
            TestMessage::DataForRoom(m) => received_data.push(m),
            TestMessage::RoomTerminated(_) => {}
        }
    }

    // Assert the MockRoomManager received DataForRoom
    assert_eq!(received_data.len(), 1);
    assert_eq!(received_data[0].room_name, "room-a");
    assert_eq!(received_data[0].data, test_data);
}

#[actix::test]
#[ntest::timeout(1000)]
async fn test_late_subscriber_republication() {
    // Test 2: Late Subscriber (Re-Publication)
    let (mock_mgr_addr, harness) = start_mock_connection_manager();

    // Subscribe first
    let (tx, _rx) = mpsc::unbounded_channel();
    let room_mgr = MockRoomManager::new(tx).start();
    mock_mgr_addr.do_send(crate::bus::SubscribeToRoom {
        room_name: "room-a".to_string(),
        room_is_active_recipient: room_mgr.clone().recipient(),
        data_recipient: room_mgr.clone().recipient(),
        termination_recipient: room_mgr.clone().recipient(),
    });

    // Simulate the room becoming active
    harness.simulate_room_is_active("room-a".to_string()).await;

    // Now, subscribe a new MockRoomManager.
    let (tx2, mut rx2) = mpsc::unbounded_channel();
    let room_mgr2 = MockRoomManager::new(tx2).start();
    mock_mgr_addr.do_send(crate::bus::SubscribeToRoom {
        room_name: "room-a".to_string(),
        room_is_active_recipient: room_mgr2.clone().recipient(),
        data_recipient: room_mgr2.clone().recipient(),
        termination_recipient: room_mgr2.clone().recipient(),
    });

    // Wait
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Collect messages for room_mgr2
    let mut received_room_is_active = Vec::new();
    while let Ok(msg) = rx2.try_recv() {
        if let TestMessage::RoomIsActive(m) = msg {
            received_room_is_active.push(m);
        }
    }

    // Assert: The subscriber should receive RoomIsActive after subscribing
    assert_eq!(received_room_is_active.len(), 1);
    assert_eq!(received_room_is_active[0].room_name, "room-a");
}

#[actix::test]
#[ntest::timeout(1000)]
async fn test_shutdown_cascade() {
    // Test 3: Shutdown Cascade
    let (mock_mgr_addr, _harness) = start_mock_connection_manager();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let room_mgr = MockRoomManager::new(tx).start();

    // Subscribe to "room-a"
    mock_mgr_addr.do_send(crate::bus::SubscribeToRoom {
        room_name: "room-a".to_string(),
        room_is_active_recipient: room_mgr.clone().recipient(),
        data_recipient: room_mgr.clone().recipient(),
        termination_recipient: room_mgr.clone().recipient(),
    });

    // Start the actor
    let transport = SimpleMockTransportActor::default().start();
    let mut subscribers = std::collections::HashMap::new();
    subscribers.insert(
        "room-a".to_string(),
        crate::bus::RoomSubscribers {
            room_is_active: room_mgr.clone().recipient::<RoomIsActive>(),
            data: room_mgr.clone().recipient::<DataForRoom>(),
            termination: room_mgr.clone().recipient::<RoomTerminated>(),
        },
    );
    let actor = ZzNetConnActor::new(
        transport.recipient(),
        subscribers,
        "1.0".to_string(),
        AuthRole::Collector,
        vec!["room-a".to_string()],
        mock_mgr_addr.clone().recipient(),
    );
    let actor_addr = actor.start();

    // Complete handshake
    let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: AuthRole::Database,
        offered_rooms: vec!["room-a".to_string()],
    });
    let hello_data = serialize(&hello_frame).unwrap();
    actor_addr.do_send(FrameFromTransport(hello_data));

    let publish_frame = Frame::Room(RoomFrame::PublishRooms {
        offered_rooms: vec!["room-a".to_string()],
    });
    let publish_data = serialize(&publish_frame).unwrap();
    actor_addr.do_send(FrameFromTransport(publish_data));

    // Wait
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Simulate transport termination
    actor_addr.do_send(TransportTerminated);

    // Wait
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Collect messages
    let mut received_termination = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        if let TestMessage::RoomTerminated(m) = msg {
            received_termination.push(m);
        }
    }

    // Assert: The mock subscriber actor receives a RoomTerminated message.
    assert_eq!(received_termination.len(), 1);
    assert_eq!(received_termination[0].room_name, "room-a");
}
