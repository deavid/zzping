//! Contains the `MockTransportManager`, the central orchestrator for the mock transport.

use super::connection::{MockTransportConnectionActor, PoisonPill};
use super::harness::HarnessCommand;
use crate::actor::ConnectionTerminated;
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
                client_conn_actor: _client_conn_actor,
                server_conn_actor: _server_conn_actor,
            } => {
                log::debug!(
                    "MockTransportManager: DISABLED - this mock is broken and needs to be redesigned"
                );
                // TODO: Fix the lazy AI design - actors should be created with transports, not vice versa
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
