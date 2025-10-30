//! # zznet-room
//!
//! This crate provides the `RoomActor<T>` abstraction for bidirectional
//! typed communication between components.
//!
//! ## Core Concept
//!
//! A `RoomActor<T>` is a typed actor that:
//! - Sends typed messages to a peer
//! - Receives typed messages from a peer
//! - Delivers to a local component handler
//!

pub mod actor;
pub mod room_manager;
pub mod room_message_trait;
