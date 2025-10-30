//! # zznet-room
//!
//! Proof of Concept #1: Room-to-Room typed communication
//!
//! This crate provides the `Room<T>` abstraction for bidirectional
//! typed communication between components without network I/O.
//!
//! ## Core Concept
//!
//! A `Room<T>` is a typed channel that:
//! - Sends typed messages to a peer
//! - Receives typed messages from a peer
//! - Delivers to a local component handler
//!
//! // In an Actix runtime:
//! // let actor_a = TestActor.start();
//! // let (room_a, channels_a) = Room::new(actor_a.recipient());
//! // let actor_b = TestActor.start();
//! // let (room_b, channels_b) = Room::new(actor_b.recipient());
//! // let connection = connect_rooms(channels_a, channels_b);
//! ```
//!

/// Connector utilities to wire two `Room`s together.
pub mod connector;

/// Typed in-memory room abstraction for local component messaging.
pub mod room;

/// Room message trait shared with components.
pub mod room_message_trait;

/// Component-provided room factory trait for Router registration.
pub mod room_manager;

/// Generic room actor for centralized (de-)serialization.
pub mod actor;

// Export public types
pub use actor::RoomActor;
pub use room::{Room, RoomChannels, RoomError, RoomRegistry, SendError};
pub use room_manager::RoomInboundRecipient;
pub use room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError};

#[cfg(test)]
mod tests;
