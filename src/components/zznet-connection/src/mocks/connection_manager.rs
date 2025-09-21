//! Contains the `MockZzNetConnManager`, a mock implementation for testing bus interface functionality.

use super::connection::SimpleMockTransportActor;
use crate::actor::{ConnectionTerminated, ZzNetConnActor};
use crate::auth::AuthRole;
use crate::bus::{RoomIsActive, SubscribeToRoom};
use actix::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
use tokio::sync::oneshot;

/// Commands for controlling the MockZzNetConnManager behavior in tests.
#[derive(Message, Debug)]
#[rtype(result = "()")]
pub enum MockConnManagerCommand {
    /// Tell the mock to simulate a room becoming active for the given name.
    SimulateRoomIsActive(String),
    /// Ask the mock how many subscribers it currently has for a given room.
    GetSubscriberCount(String, oneshot::Sender<usize>),
}

/// A mock actor that simulates the bus interface functionality of `ZzNetConnManager`.
///
/// This mock handles room subscription logic and room activation notifications,
/// but does NOT handle transport-level concerns like connection creation or lifecycle.
#[derive(Default)]
pub struct MockZzNetConnManager {
    /// Subscribers for room activation notifications.
    subscribers: HashMap<String, Recipient<RoomIsActive>>,
    /// Active rooms that have been simulated.
    active_rooms: HashSet<String>,
}

impl Actor for MockZzNetConnManager {
    type Context = Context<Self>;
}

/// Handles commands from the test harness for controlling mock behavior.
impl Handler<MockConnManagerCommand> for MockZzNetConnManager {
    type Result = ();

    fn handle(&mut self, msg: MockConnManagerCommand, ctx: &mut Context<Self>) {
        match msg {
            MockConnManagerCommand::SimulateRoomIsActive(room_name) => {
                self.active_rooms.insert(room_name.clone());
                if let Some(subscriber) = self.subscribers.get(&room_name) {
                    log::debug!(
                        "MockZzNetConnManager simulating RoomIsActive for '{}'",
                        room_name
                    );
                    // Create a dummy connection actor with simple transport
                    let dummy_transport = SimpleMockTransportActor::default().start();
                    let dummy_conn_actor = ZzNetConnActor::new(
                        dummy_transport.recipient(),
                        HashMap::new(),
                        "1.0".to_string(),
                        AuthRole::Collector,
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
            MockConnManagerCommand::GetSubscriberCount(room_name, sender) => {
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

/// Handles connection termination notifications.
impl Handler<ConnectionTerminated> for MockZzNetConnManager {
    type Result = ();

    fn handle(&mut self, msg: ConnectionTerminated, _ctx: &mut Context<Self>) {
        log::info!("Mock connection terminated: {:?}", msg.connection_actor);
    }
}

/// Handles room subscription requests.
impl Handler<SubscribeToRoom> for MockZzNetConnManager {
    type Result = ();

    fn handle(&mut self, msg: SubscribeToRoom, ctx: &mut Context<Self>) {
        log::debug!(
            "MockZzNetConnManager received subscription for room '{}'",
            msg.room_name
        );
        self.subscribers
            .insert(msg.room_name.clone(), msg.room_is_active_recipient.clone());
        // If the room is already active, immediately send RoomIsActive
        if self.active_rooms.contains(&msg.room_name) {
            // Create a dummy connection actor with simple transport
            let dummy_transport = SimpleMockTransportActor::default().start();
            let dummy_conn_actor = ZzNetConnActor::new(
                dummy_transport.recipient(),
                HashMap::new(),
                "1.0".to_string(),
                AuthRole::Collector,
                vec![],
                ctx.address().recipient(),
            )
            .start();

            msg.room_is_active_recipient.do_send(RoomIsActive {
                room_name: msg.room_name,
                connection_actor: dummy_conn_actor,
            });
        }
    }
}
