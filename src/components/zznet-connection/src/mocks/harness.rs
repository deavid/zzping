//! Defines the test harness for controlling the mock transport layer.
//!
//! The `MockTransportHarness` is the primary tool a test will use to interact
//! with and drive the behavior of the simulated network.

use super::connection_manager::{MockConnManagerCommand, MockZzNetConnManager};
use super::manager::MockTransportManager;
use crate::actor::ZzNetConnActor;
use actix::prelude::*;
use tokio::sync::oneshot;

/// A command sent from a test harness to the `MockTransportManager` to
/// instruct it to perform an action.
#[derive(Message, Debug)]
#[rtype(result = "()")]
pub enum HarnessCommand {
    /// Instructs the mock manager to simulate a new, successful connection
    /// between two peer `ZzNetConnActor`s.
    SimulateConnection {
        /// The address of the "client" side ZzNetConnActor.
        client_conn_actor: Addr<ZzNetConnActor>,
        /// The address of the "server" side ZzNetConnActor.
        server_conn_actor: Addr<ZzNetConnActor>,
    },
    /// Instructs the mock manager to shut down all active connections.
    Shutdown,
}

/// The public-facing test harness.
///
/// An instance of this struct is returned by the `MockHarnessFactory`. It provides
/// methods to control the mock network during a test.
pub struct MockTransportHarness {
    /// The address of the `MockTransportManager` actor that this harness controls.
    manager: Addr<MockTransportManager>,
}

impl MockTransportHarness {
    /// Creates a new harness that controls the given manager.
    /// This is typically called by the `MockHarnessFactory`.
    pub fn new(manager: Addr<MockTransportManager>) -> Self {
        Self { manager }
    }

    /// Simulates a new, successful connection between two components.
    ///
    /// This will cause the mock transport layer to create a pair of connected
    /// `MockTransportConnectionActor`s and deliver their handles to the
    /// provided client and server `ZzNetConnActor`s.
    pub async fn simulate_connection(
        &self,
        client_conn_actor: Addr<ZzNetConnActor>,
        server_conn_actor: Addr<ZzNetConnActor>,
    ) {
        self.manager.do_send(HarnessCommand::SimulateConnection {
            client_conn_actor,
            server_conn_actor,
        });
    }

    /// Shuts down all active mock connections.
    ///
    /// This will terminate all `MockTransportConnectionActor`s that are currently
    /// active, ensuring no zombie actors remain after a test.
    pub async fn shutdown(&self) {
        self.manager.do_send(HarnessCommand::Shutdown);
    }
}

/// The public-facing test harness for controlling bus interface mocking.
///
/// This harness controls a `MockZzNetConnManager` and provides methods to
/// simulate room activation and query subscription state.
pub struct MockBusHarness {
    /// The address of the `MockZzNetConnManager` actor that this harness controls.
    manager: Addr<MockZzNetConnManager>,
}

impl MockBusHarness {
    /// Creates a new harness that controls the given manager.
    /// This is typically called by the `MockHarnessFactory`.
    pub fn new(manager: Addr<MockZzNetConnManager>) -> Self {
        Self { manager }
    }

    /// Simulate a room becoming active (for connection manager functionality).
    pub async fn simulate_room_is_active(&self, room_name: String) {
        self.manager.do_send(MockConnManagerCommand::SimulateRoomIsActive(room_name));
    }

    /// Get the number of subscribers for a room (for connection manager functionality).
    pub async fn get_subscriber_count(&self, room_name: &str) -> usize {
        let (tx, rx) = oneshot::channel();
        self.manager.do_send(MockConnManagerCommand::GetSubscriberCount(
            room_name.to_string(),
            tx,
        ));
        rx.await.unwrap_or(0)
    }
}
