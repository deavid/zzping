use actix::prelude::*;
use std::time::Duration;
use zznet_connection::actor::{FrameFromTransport, ZzNetConnActor};
use zznet_connection::bus::RoomIsActive;
use zznet_connection::mocks::start_mock_connection_manager;
use zznet_connection::protocol::{Frame, HandshakeFrame, RoomFrame, serialize};

#[derive(Default)]
struct MockRoomManager {
    received: Vec<RoomIsActive>,
}

impl Actor for MockRoomManager {
    type Context = Context<Self>;
}

impl Handler<RoomIsActive> for MockRoomManager {
    type Result = ();

    fn handle(&mut self, msg: RoomIsActive, _ctx: &mut Context<Self>) {
        self.received.push(msg);
    }
}

#[actix::test]
#[ntest::timeout(1000)]
async fn test_full_handshake_and_notification() {
    let (mock_mgr_addr, _harness) = start_mock_connection_manager();
    let room_mgr = MockRoomManager::default().start();

    // Subscribe to "intent-config"
    mock_mgr_addr.do_send(zznet_connection::bus::SubscribeToRoom {
        room_name: "intent-config".to_string(),
        subscriber: room_mgr.clone().recipient(),
    });

    // Start the actor manually
    let transport = zznet_connection::actor::DummyTransportActor.start();
    let mut subscribers = std::collections::HashMap::new();
    subscribers.insert(
        "intent-config".to_string(),
        room_mgr.clone().recipient::<RoomIsActive>(),
    );
    let actor = ZzNetConnActor::new(
        transport,
        subscribers,
        "1.0".to_string(),
        "client".to_string(),
        vec!["intent-config".to_string()],
        mock_mgr_addr.clone().recipient(),
    );
    let actor_addr = actor.start();

    // Send Hello from peer
    let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: "server".to_string(),
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
    let transport = zznet_connection::actor::DummyTransportActor.start();
    let subscribers = std::collections::HashMap::new();
    let actor = ZzNetConnActor::new(
        transport,
        subscribers,
        "1.0".to_string(),
        "client".to_string(),
        vec!["intent-config".to_string()],
        mock_mgr_addr.clone().recipient(),
    );
    let actor_addr = actor.start();

    // Complete handshake
    let hello_frame = Frame::Handshake(HandshakeFrame::Hello {
        protocol_version: "1.0".to_string(),
        auth_role: "server".to_string(),
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
    mock_mgr_addr
        .clone()
        .do_send(zznet_connection::bus::SubscribeToRoom {
            room_name: "intent-config".to_string(),
            subscriber: room_mgr.clone().recipient(),
        });

    // Wait
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send PublishRooms again from peer
    actor_addr.do_send(FrameFromTransport(publish_data));

    // Wait
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Check
}
