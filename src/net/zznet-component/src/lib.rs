//! Generic component abstractions for ZZNet network components.
//!
//! This crate provides the building blocks for creating network-enabled components
//! that follow the three-actor pattern:
//! - MainActor (business logic)
//! - NetworkManager (peer lifecycle, using GenericNetworkManager)
//! - NetworkActor (per-peer translator)
//!
//! Components implement the `NetComponent` trait to define their types, and then
//! use `GenericNetworkManager` and `GenericRoomFactory` to handle all the boilerplate
//! of peer management, room creation, and actor wiring.

mod network_manager;
mod room_factory;
mod traits;

pub use network_manager::GenericNetworkManager;
pub use room_factory::GenericRoomFactory;
pub use traits::NetComponent;
