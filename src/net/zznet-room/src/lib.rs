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

// Export public types
pub use room::{Room, RoomChannels, RoomError, RoomRegistry, SendError};

#[cfg(test)]
mod tests;
