//! Provides the public factory for creating a complete mock transport setup.

use super::connection_manager::MockZzNetConnManager;
use super::harness::{MockBusHarness, MockTransportHarness};
use super::manager::MockTransportManager;
use actix::prelude::*;

/// A factory for creating and starting mock components for testing.
///
/// This factory provides methods to create separate mock components for
/// transport simulation and bus interface mocking.
#[derive(Default, Debug)]
pub struct MockHarnessFactory;

impl MockHarnessFactory {
    /// Spawns a new `MockTransportManager` actor and returns its address along
    /// with a `MockTransportHarness` to control transport simulation.
    ///
    /// This method consumes the factory to ensure it's used once per setup.
    ///
    /// # Returns
    /// A tuple containing:
    ///  - `Addr<MockTransportManager>`: The handle to the running mock transport manager.
    ///  - `MockTransportHarness`: The harness object for controlling transport simulation.
    pub fn transport_manager(self) -> (Addr<MockTransportManager>, MockTransportHarness) {
        // Spawn the transport manager actor.
        let manager_addr = MockTransportManager::start_default();

        // Create the harness that will control it.
        let harness = MockTransportHarness::new(manager_addr.clone());

        // Return both to the test.
        (manager_addr, harness)
    }

    /// Spawns a new `MockZzNetConnManager` actor and returns its address along
    /// with a `MockBusHarness` to control bus interface mocking.
    ///
    /// This method consumes the factory to ensure it's used once per setup.
    ///
    /// # Returns
    /// A tuple containing:
    ///  - `Addr<MockZzNetConnManager>`: The handle to the running mock connection manager.
    ///  - `MockBusHarness`: The harness object for controlling bus interface simulation.
    pub fn connection_manager(self) -> (Addr<MockZzNetConnManager>, MockBusHarness) {
        // Spawn the connection manager actor.
        let manager_addr = MockZzNetConnManager::start_default();

        // Create the harness that will control it.
        let harness = MockBusHarness::new(manager_addr.clone());

        // Return both to the test.
        (manager_addr, harness)
    }
}

/// A helper function to easily set up the mock transport manager and its harness for a test.
/// This provides a unified interface for transport mocking.
pub fn start_mock_transport_manager() -> (Addr<MockTransportManager>, MockTransportHarness) {
    MockHarnessFactory.transport_manager()
}

/// A helper function to easily set up the mock connection manager and its harness for a test.
/// This provides a unified interface for bus interface mocking.
pub fn start_mock_connection_manager() -> (Addr<MockZzNetConnManager>, MockBusHarness) {
    MockHarnessFactory.connection_manager()
}
