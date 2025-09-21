//! Provides mock implementations and test harnesses for the `zznet` actor stack.
//!
//! This crate is intended for use in `[dev-dependencies]` and enables hermetic,
//! in-memory integration testing of network-aware components.
//!
//! ## Overview
//!
//! The mock system is split into two separate components for proper separation of concerns:
//!
//! ### Transport Layer Mocking
//! - **`MockTransportManager`**: Simulates the transport layer with full-duplex communication
//! - **`MockTransportHarness`**: Test control interface for transport simulation
//! - Creates connected pairs of `MockTransportConnectionActor`s to simulate network links
//!
//! ### Bus Interface Mocking
//! - **`MockZzNetConnManager`**: Simulates the connection manager's bus interface
//! - **`MockBusHarness`**: Test control interface for bus interface simulation
//! - Handles room subscription logic and room activation notifications
//!
//! ## Usage in Tests
//!
//! ### Transport Testing
//! ```rust
//! use zznet_connection::mocks::{MockHarnessFactory, SimpleMockTransportActor};
//! use zznet_connection::actor::ZzNetConnActor;
//! use zznet_connection::auth::AuthRole;
//! use actix::prelude::*;
//! use std::collections::HashMap;
//! use tokio;
//!
//! #[actix::main]
//! async fn main() {
//!     // Create the transport mock harness
//!     let (transport_mgr, transport_harness) = MockHarnessFactory.transport_manager();
//!
//!     // Create ZzNetConnActors with proper initialization
//!     let dummy_transport = SimpleMockTransportActor::default().start();
//!     let client_actor = ZzNetConnActor::new(
//!         dummy_transport.recipient(),
//!         HashMap::new(),
//!         "1.0".to_string(),
//!         AuthRole::Collector,
//!         vec![],
//!         transport_mgr.clone().recipient(),
//!     ).start();
//!
//!     let dummy_transport2 = SimpleMockTransportActor::default().start();
//!     let server_actor = ZzNetConnActor::new(
//!         dummy_transport2.recipient(),
//!         HashMap::new(),
//!         "1.0".to_string(),
//!         AuthRole::Collector,
//!         vec![],
//!         transport_mgr.recipient(),
//!     ).start();
//!
//!     // Simulate a connection
//!     transport_harness.simulate_connection(client_actor, server_actor).await;
//!
//!     // Clean up
//!     transport_harness.shutdown().await;
//! }
//! ```
//!
//! ### Bus Interface Testing
//! ```rust
//! use zznet_connection::mocks::{MockHarnessFactory, SimpleMockTransportActor};
//! use zznet_connection::bus::{SubscribeToRoom, RoomIsActive, DataForRoom, RoomTerminated};
//! use zznet_connection::actor::ZzNetConnActor;
//! use zznet_connection::auth::AuthRole;
//! use actix::prelude::*;
//! use std::collections::HashMap;
//! use tokio;
//!
//! // Mock actor to receive bus messages
//! #[derive(Default)]
//! struct MockReceiver;
//!
//! impl Actor for MockReceiver {
//!     type Context = Context<Self>;
//! }
//!
//! impl Handler<RoomIsActive> for MockReceiver {
//!     type Result = ();
//!     fn handle(&mut self, _msg: RoomIsActive, _ctx: &mut Context<Self>) {}
//! }
//!
//! impl Handler<DataForRoom> for MockReceiver {
//!     type Result = ();
//!     fn handle(&mut self, _msg: DataForRoom, _ctx: &mut Context<Self>) {}
//! }
//!
//! impl Handler<RoomTerminated> for MockReceiver {
//!     type Result = ();
//!     fn handle(&mut self, _msg: RoomTerminated, _ctx: &mut Context<Self>) {}
//! }
//!
//! #[actix::main]
//! async fn main() {
//!     // Create the bus interface mock harness
//!     let (bus_mgr, bus_harness) = MockHarnessFactory.connection_manager();
//!
//!     // Create a mock receiver for bus messages
//!     let receiver = MockReceiver::default().start();
//!
//!     // Subscribe to rooms through the bus interface
//!     bus_mgr.do_send(SubscribeToRoom {
//!         room_name: "room-name".to_string(),
//!         room_is_active_recipient: receiver.clone().recipient(),
//!         data_recipient: receiver.clone().recipient(),
//!         termination_recipient: receiver.recipient(),
//!     });
//!
//!     // Simulate room activation
//!     bus_harness.simulate_room_is_active("room-name".to_string()).await;
//!
//!     // Check subscriber count
//!     let count = bus_harness.get_subscriber_count("room-name").await;
//!     assert_eq!(count, 1);
//! }
//! ```
//!
//! ## Architecture
//!
//! - `MockHarnessFactory`: Entry point for creating separate mock setups
//! - `MockTransportManager` & `MockTransportHarness`: Transport layer simulation
//! - `MockZzNetConnManager` & `MockBusHarness`: Bus interface simulation
//! - `MockTransportConnectionActor`: Simulates individual network connections
//! - `ZzNetConnActor`: Trait/interface for components using the transport

pub mod connection;
pub mod connection_manager;
pub mod factory;
pub mod harness;
pub mod manager;

// Re-export commonly used items
pub use connection::{MockTransportConnectionActor, SimpleMockTransportActor};
pub use connection_manager::{MockConnManagerCommand, MockZzNetConnManager};
pub use factory::{
    MockHarnessFactory, start_mock_connection_manager, start_mock_transport_manager,
};
pub use harness::{HarnessCommand, MockBusHarness, MockTransportHarness};
pub use manager::MockTransportManager;
