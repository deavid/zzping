//! Common test utilities and helpers for zznet-connection tests

use crate::actor::ZzNetConnActor;
use crate::auth::AuthRole;
use crate::bus::{DataForRoom, RoomIsActive, RoomTerminated, SubscribeToRoom};
use crate::mocks::start_mock_connection_manager;
use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Setup logger for tests - must be called at start of every test
pub(crate) fn setup_logger() {
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();
}

/// Test message types for capturing bus events
#[derive(Clone, Debug)]
pub enum TestMessage {
    RoomIsActive(RoomIsActive),
    DataForRoom(DataForRoom),
    RoomTerminated(RoomTerminated),
}

/// Mock room manager that captures all bus messages for verification
#[derive(Default)]
pub struct MockRoomManager {
    sender: Option<mpsc::UnboundedSender<TestMessage>>,
}

impl MockRoomManager {
    pub fn new(sender: mpsc::UnboundedSender<TestMessage>) -> Self {
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

/// Helper to create a ZzNetConnActor with mock manager
pub fn create_test_actor(
    auth_role: AuthRole,
    offered_rooms: Vec<String>,
    subscribers: HashMap<String, crate::bus::RoomSubscribers>,
) -> Addr<ZzNetConnActor> {
    let (mock_mgr_addr, _) = start_mock_connection_manager();
    let transport = crate::mocks::SimpleMockTransportActor::default().start();

    ZzNetConnActor::new(
        transport.recipient(),
        subscribers,
        "1.0".to_string(),
        auth_role,
        offered_rooms,
        mock_mgr_addr.recipient(),
    )
    .start()
}

/// Helper to subscribe a mock room manager to a specific room
pub fn subscribe_mock_to_room(
    manager_addr: &Addr<crate::mocks::MockZzNetConnManager>,
    room_name: &str,
    mock_room_mgr: &Addr<MockRoomManager>,
) {
    manager_addr.do_send(SubscribeToRoom {
        room_name: room_name.to_string(),
        room_is_active_recipient: mock_room_mgr.clone().recipient(),
        data_recipient: mock_room_mgr.clone().recipient(),
        termination_recipient: mock_room_mgr.clone().recipient(),
    });
}

/// Helper to wait for and collect messages from a test receiver
pub async fn collect_messages(
    rx: &mut mpsc::UnboundedReceiver<TestMessage>,
    timeout_ms: u64,
) -> Vec<TestMessage> {
    let mut messages = Vec::new();
    let timeout = tokio::time::Duration::from_millis(timeout_ms);
    let start = tokio::time::Instant::now();

    while start.elapsed() < timeout {
        match tokio::time::timeout(tokio::time::Duration::from_millis(5), rx.recv()).await {
            Ok(Some(msg)) => messages.push(msg),
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    messages
}

/// Helper to filter messages by type
pub fn filter_room_active_messages(messages: &[TestMessage]) -> Vec<&RoomIsActive> {
    messages
        .iter()
        .filter_map(|msg| {
            if let TestMessage::RoomIsActive(m) = msg {
                Some(m)
            } else {
                None
            }
        })
        .collect()
}

pub fn filter_data_messages(messages: &[TestMessage]) -> Vec<&DataForRoom> {
    messages
        .iter()
        .filter_map(|msg| {
            if let TestMessage::DataForRoom(m) = msg {
                Some(m)
            } else {
                None
            }
        })
        .collect()
}

pub fn filter_termination_messages(messages: &[TestMessage]) -> Vec<&RoomTerminated> {
    messages
        .iter()
        .filter_map(|msg| {
            if let TestMessage::RoomTerminated(m) = msg {
                Some(m)
            } else {
                None
            }
        })
        .collect()
}
