//! Provides mock implementations and test harnesses for the `zznet` actor stack.
//!
//! This crate is intended for use in `[dev-dependencies]` and enables hermetic,
//! in-memory integration testing of network-aware components.
//!
//! ## Overview
//!
//! The mock transport simulates a perfect network link with the following characteristics:
//!
//! - **Full-duplex communication**: Bidirectional data flow using paired bounded channels (128 slot buffer)
//! - **Connection simulation**: Creates connected pairs of `MockTransportConnectionActor`s
//! - **Actor-based architecture**: Uses Actix actors for concurrency and message passing
//! - **Lifecycle management**: Proper startup, operation, and shutdown of mock connections
//!
//! ## Limitations and Constraints
//!
//! This mock simulates a perfect network link. It does **not** simulate:
//!
//! - **Network latency or delays**: All data transfer is instantaneous
//! - **Packet loss or reordering**: No data is dropped or reordered
//! - **Connection failures or drops mid-stream**: Connections remain stable once established
//! - **Backpressure from finite network buffers**: Uses bounded channels with 128 slots, but does not simulate network buffer limits
//! - **The zznet handshake protocol**: Only provides the raw transport layer for it
//! - **Address or peer information**: No simulation of network addresses, ports, or peer metadata
//! - **Network conditions**: No bandwidth throttling, jitter, or other network impairments
//!
//! ## Usage in Tests
//!
//! ```rust,ignore
//! use zznet_mocks::{MockHarnessFactory, ZzNetConnActor};
//!
//! // Create the mock harness
//! let (manager_addr, harness) = MockHarnessFactory.start();
//!
//! // Create your ZzNetConnActors (real or mock implementations)
//! let client_actor = ZzNetConnActor.start();
//! let server_actor = ZzNetConnActor.start();
//!
//! // Simulate a connection
//! harness.simulate_connection(client_actor, server_actor).await;
//!
//! // Run your test logic...
//!
//! // Clean up
//! harness.shutdown().await;
//! ```
//!
//! ## Architecture
//!
//! - `MockHarnessFactory`: Entry point for creating test setups
//! - `MockTransportHarness`: Test control interface
//! - `MockTransportManager`: Orchestrates connection creation and lifecycle
//! - `MockTransportConnectionActor`: Simulates individual network connections
//! - `ZzNetConnActor`: Trait/interface for components using the transport
//!
//! For tests requiring realistic network conditions, consider using real network
//! transports or advanced network simulation tools.

pub mod connection;
pub mod factory;
pub mod harness;
pub mod manager;

// Re-export commonly used items
pub use connection::{MockTransportConnectionActor, SimpleMockTransportActor};
pub use factory::start_mock_connection_manager;
pub use harness::{HarnessCommand, MockTransportHarness};
pub use manager::{MockBusCommand, MockTransportManager};
