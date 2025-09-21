//! Contains the `MockTransportManager`, the central orchestrator for the mock transport.

use super::connection::{MockTransportConnectionActor, PoisonPill, SimpleMockTransportActor};
use super::harness::HarnessCommand;
use crate::actor::{ConnectionTerminated, NewTransportConnection, ZzNetConnActor};
use crate::bus::{RoomIsActive, SubscribeToRoom};
use actix::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
use tokio::sync::oneshot;

// --- Transport Mock Commands ---

/// A command sent from a test to the MockTransportManager to drive its behavior.
#[derive(Message, Debug)]
#[rtype(result = "()")]
pub enum MockBusCommand {
    /// Tell the mock to simulate a room becoming active for the given name.
    SimulateRoomIsActive(String),
    /// Ask the mock how many subscribers it currently has for a given room.
    GetSubscriberCount(String, oneshot::Sender<usize>),
}

// --- Mock Manager Actor Implementation ---

/// A mock actor that simulates the `TransportManager`.
///
/// It listens for commands from a `MockTransportHarness` and creates
/// connected pairs of `MockTransportConnectionActor`s to simulate a network link.
/// It can also function as a mock connection manager for testing the bus interface.
#[derive(Default)]
pub struct MockTransportManager {
    /// Tracks active connection actors for lifecycle management.
    active_connections: HashMap<usize, Addr<MockTransportConnectionActor>>,
    /// Counter for assigning unique IDs to connections.
    connection_counter: usize,
    /// Subscribers for the connection manager mock functionality.
    subscribers: HashMap<String, Recipient<RoomIsActive>>,
    /// Active rooms for the connection manager mock functionality.
    active_rooms: HashSet<String>,
}

impl Actor for MockTransportManager {
    type Context = Context<Self>;
}

/// Handles commands from the test harness.
impl Handler<HarnessCommand> for MockTransportManager {
    type Result = ();

    fn handle(&mut self, msg: HarnessCommand, _ctx: &mut Context<Self>) {
        match msg {
            HarnessCommand::SimulateConnection {
                client_conn_actor,
                server_conn_actor,
            } => {
                log::debug!("MockTransportManager: Simulating a new connection.");

                // Create two pairs of bounded channels to simulate a full-duplex connection.
                // client_tx sends data from the "client" side to the "server" side.
                // server_tx sends data from the "server" side to the "client" side.
                let (client_tx, client_rx) = tokio::sync::mpsc::channel(128);
                let (server_tx, server_rx) = tokio::sync::mpsc::channel(128);

                // Spawn the "server" side of the connection.
                let server_conn_addr = MockTransportConnectionActor::new(
                    server_tx,
                    client_rx,
                    server_conn_actor.clone(),
                )
                .start();

                // Spawn the "client" side of the connection.
                let client_conn_addr = MockTransportConnectionActor::new(
                    client_tx,
                    server_rx,
                    client_conn_actor.clone(),
                )
                .start();

                // Track the connections
                let conn_id = self.connection_counter;
                self.connection_counter += 1;
                self.active_connections
                    .insert(conn_id, client_conn_addr.clone());
                self.active_connections
                    .insert(conn_id + 1, server_conn_addr.clone());

                // Now, notify the real `ZzNetConnActor`s that their new transport
                // connections are ready.

                // The client ZzNetConnActor gets the client-side transport handle.
                client_conn_actor.do_send(NewTransportConnection {
                    transport_handle: client_conn_addr.recipient(),
                });

                // The server ZzNetConnActor gets the server-side transport handle.
                server_conn_actor.do_send(NewTransportConnection {
                    transport_handle: server_conn_addr.recipient(),
                });
            }
            HarnessCommand::Shutdown => {
                log::debug!("MockTransportManager: Shutting down all connections.");
                // Stop all active connections
                for (_id, conn_addr) in self.active_connections.drain() {
                    conn_addr.do_send(PoisonPill);
                }
                self.connection_counter = 0;
            }
        }
    }
}

/// Handles bus commands for connection manager functionality.
impl Handler<MockBusCommand> for MockTransportManager {
    type Result = ();

    fn handle(&mut self, msg: MockBusCommand, ctx: &mut Context<Self>) {
        match msg {
            MockBusCommand::SimulateRoomIsActive(room_name) => {
                self.active_rooms.insert(room_name.clone());
                if let Some(subscriber) = self.subscribers.get(&room_name) {
                    log::debug!(
                        "MockTransportManager simulating RoomIsActive for '{}'",
                        room_name
                    );
                    // Create a dummy connection actor with simple transport
                    let dummy_transport = SimpleMockTransportActor::default().start();
                    let dummy_conn_actor = ZzNetConnActor::new(
                        dummy_transport.recipient(),
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

/// Handles room subscription for connection manager functionality.
impl Handler<SubscribeToRoom> for MockTransportManager {
    type Result = ();

    fn handle(&mut self, msg: SubscribeToRoom, ctx: &mut Context<Self>) {
        log::debug!(
            "MockTransportManager received subscription for room '{}'",
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
                "client".to_string(),
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

/// Handles connection termination notifications.
impl Handler<ConnectionTerminated> for MockTransportManager {
    type Result = ();

    fn handle(&mut self, msg: ConnectionTerminated, _ctx: &mut Context<Self>) {
        log::info!("Mock connection terminated: {:?}", msg.connection_actor);
    }
}
