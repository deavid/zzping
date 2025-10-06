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
//! ## Example
//!
//! ```rust
//! use zznet_room::room::Room;
//! use zznet_room::connector::connect_rooms;
//! use actix::prelude::*;
//!
//! #[derive(Message, Clone)]
//! #[rtype(result = "()")]
//! struct TestMsg { value: i32 }
//!
//! struct TestActor;
//! impl Actor for TestActor { type Context = Context<Self>; }
//! impl Handler<TestMsg> for TestActor {
//!     type Result = ();
//!     fn handle(&mut self, msg: TestMsg, _: &mut Context<Self>) {}
//! }
//!
//! // In an Actix runtime:
//! // let actor_a = TestActor.start();
//! // let (room_a, channels_a) = Room::new(actor_a.recipient());
//! // let actor_b = TestActor.start();
//! // let (room_b, channels_b) = Room::new(actor_b.recipient());
//! // let connection = connect_rooms(channels_a, channels_b);
//! ```
//!
//! ## Testing
//!
//! Rooms support both automatic and manual message processing:
//!
//! ```rust
//! // Automatic: spawn background receiver
//! // room.spawn_receiver().unwrap();
//!
//! // Manual: process one message at a time (for tests)
//! // room.process_one().await.unwrap();
//! ```

/// Connector utilities to wire two `Room`s together.
pub mod connector;

/// Typed in-memory room abstraction for local component messaging.
pub mod room;

#[cfg(test)]
mod tests;
