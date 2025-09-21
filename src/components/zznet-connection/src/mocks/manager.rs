//! Contains the `MockTransportManager`, the central orchestrator for the mock transport.

use super::connection::{MockTransportConnectionActor, PoisonPill};
use super::harness::HarnessCommand;
use crate::actor::{ConnectionTerminated, NewTransportConnection};
use actix::prelude::*;
use std::collections::HashMap;

// --- Transport Mock Commands ---

// --- Mock Manager Actor Implementation ---

/// A mock actor that simulates the `TransportManager`.
///
/// It listens for commands from a `MockTransportHarness` and creates
/// connected pairs of `MockTransportConnectionActor`s to simulate a network link.
#[derive(Default)]
pub struct MockTransportManager {
    /// Tracks active connection actors for lifecycle management.
    active_connections: HashMap<usize, Addr<MockTransportConnectionActor>>,
    /// Counter for assigning unique IDs to connections.
    connection_counter: usize,
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

/// Handles connection termination notifications.
impl Handler<ConnectionTerminated> for MockTransportManager {
    type Result = ();

    fn handle(&mut self, msg: ConnectionTerminated, _ctx: &mut Context<Self>) {
        log::info!("Mock connection terminated: {:?}", msg.connection_actor);
    }
}
