//! Provides the public factory for creating a complete mock transport setup.

use super::harness::MockTransportHarness;
use super::manager::MockTransportManager;
use actix::prelude::*;

/// A factory for creating and starting a `MockTransportManager` and its
/// associated `MockTransportHarness`.
///
/// This is the primary, user-facing entry point for setting up a `zznet`
/// integration test.
#[derive(Default, Debug)]
pub struct MockHarnessFactory;

impl MockHarnessFactory {
    /// Spawns a new `MockTransportManager` actor and returns its address along
    /// with a `MockTransportHarness` to control it.
    ///
    /// This method consumes the factory to ensure it's used once per setup.
    ///
    /// # Returns
    /// A tuple containing:
    ///  - `Addr<MockTransportManager>`: The handle to the running mock manager.
    ///    This should be injected as a dependency into the component under test
    ///    (e.g., the real `ZzNetConnManager`).
    ///  - `MockTransportHarness`: The harness object that the test itself will
    ///    use to drive the simulation (e.g., to simulate new connections).
    pub fn start(self) -> (Addr<MockTransportManager>, MockTransportHarness) {
        // 1. Spawn the manager actor.
        let manager_addr = MockTransportManager::start_default();

        // 2. Create the harness that will control it.
        let harness = MockTransportHarness::new(manager_addr.clone());

        // 3. Return both to the test.
        (manager_addr, harness)
    }
}

/// A helper function to easily set up the mock manager and its harness for a test.
/// This provides a unified interface that can be used both for transport mocking
/// and connection manager mocking.
pub fn start_mock_connection_manager() -> (Addr<MockTransportManager>, MockTransportHarness) {
    MockHarnessFactory.start()
}
