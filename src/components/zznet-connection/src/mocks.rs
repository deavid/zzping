//! Contains mock actors and test harnesses for consumers of the `zznet-connection` crate.
//!
//! These tools allow higher-level crates (like `zznet-room`) to test their
//! interaction with the connection layer without needing to run a real
//! transport or connection manager.

use crate::actor::{ConnectionTerminated, DummyTransportActor, ZzNetConnActor};
use crate::bus::{RoomIsActive, SubscribeToRoom};
use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::oneshot;

/// A command sent from a test to the MockConnectionManager to drive its behavior.
#[derive(Message, Debug)]
#[rtype(result = "()")]
pub enum MockBusCommand {
    /// Tell the mock to simulate a room becoming active for the given name.
    SimulateRoomIsActive(String),
    /// Ask the mock how many subscribers it currently has for a given room.
    GetSubscriberCount(String, oneshot::Sender<usize>),
}

/// A harness for controlling and inspecting the MockConnectionManager from a test.
pub struct MockConnectionManagerHarness {
    /// Send commands to the mock manager to trigger simulated events.
    pub command_tx: Addr<MockConnectionManager>,
}

impl MockConnectionManagerHarness {
    /// A helper method for synchronously asking the mock for its subscriber count.
    pub async fn get_subscriber_count(&self, room_name: &str) -> usize {
        let (tx, rx) = oneshot::channel();
        self.command_tx.do_send(MockBusCommand::GetSubscriberCount(
            room_name.to_string(),
            tx,
        ));
        rx.await.unwrap_or(0)
    }
}

/// A mock version of the `ZzNetConnManager` for use in unit and integration tests.
#[derive(Default)]
pub struct MockConnectionManager {
    subscribers: HashMap<String, Recipient<RoomIsActive>>,
}

impl Actor for MockConnectionManager {
    type Context = Context<Self>;
}

/// The mock only needs to implement the public bus interface. It doesn't need to
/// handle messages from a transport layer.
impl Handler<SubscribeToRoom> for MockConnectionManager {
    type Result = ();

    fn handle(&mut self, msg: SubscribeToRoom, _ctx: &mut Context<Self>) {
        log::debug!(
            "MockConnectionManager received subscription for room '{}'",
            msg.room_name
        );
        self.subscribers.insert(msg.room_name, msg.subscriber);
    }
}

/// The mock also handles test-specific commands.
impl Handler<MockBusCommand> for MockConnectionManager {
    type Result = ();

    fn handle(&mut self, msg: MockBusCommand, ctx: &mut Context<Self>) {
        match msg {
            MockBusCommand::SimulateRoomIsActive(room_name) => {
                if let Some(subscriber) = self.subscribers.get(&room_name) {
                    log::debug!(
                        "MockConnectionManager simulating RoomIsActive for '{}'",
                        room_name
                    );
                    // In a test, we don't have a real connection actor, so we spawn a dummy
                    // one to provide a valid handle.
                    let dummy_transport = DummyTransportActor.start();
                    let dummy_conn_actor = ZzNetConnActor::new(
                        dummy_transport,
                        HashMap::new(),
                        "1.0".to_string(),
                        "client".to_string(),
                        vec![],
                        ctx.address().recipient(),
                    )
                    .start();
                    subscriber.do_send(RoomIsActive {
                        room_name,
                        connection_actor: dummy_conn_actor,
                    });
                } else {
                    log::warn!(
                        "Mock received SimulateRoomIsActive for a room with no subscribers: '{}'",
                        room_name
                    );
                }
            }
            MockBusCommand::GetSubscriberCount(room_name, sender) => {
                let count = self
                    .subscribers
                    .get(&room_name)
                    .map(|_| 1) // Simplified to 1 subscriber for now
                    .unwrap_or(0);
                let _ = sender.send(count);
            }
        }
    }
}

impl Handler<ConnectionTerminated> for MockConnectionManager {
    type Result = ();

    fn handle(&mut self, msg: ConnectionTerminated, _ctx: &mut Context<Self>) {
        log::info!("Mock connection terminated: {:?}", msg.connection_actor);
    }
}

/// A helper function to easily set up the mock manager and its harness for a test.
pub fn start_mock_connection_manager() -> (Addr<MockConnectionManager>, MockConnectionManagerHarness)
{
    let mock_manager_addr = MockConnectionManager::default().start();
    let harness = MockConnectionManagerHarness {
        command_tx: mock_manager_addr.clone(),
    };
    (mock_manager_addr, harness)
}
