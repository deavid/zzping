//! # zznet-hello
//!
//! HELLO protocol handler and serialization boundary for the ZZPing network stack.
//!
//! This crate sits between the transport layer (bytes) and the session layer (typed messages),
//! handling:
//! - **Protocol A (HELLO)**: Peer identity exchange and initial handshake
//! - **Serialization**: Converting typed messages to/from bytes
//! - **Room negotiation**: Computing intersection of offered rooms
//!
//! ## Architecture Position
//!
//! ```text
//! SessionManager (typed messages)
//!       ↕
//! [ HELLO Handler ] ← This crate (serialization boundary)
//!       ↕
//! Transport (bytes)
//! ```
//!
//! ## Two-Phase Protocol
//!
//! ### Phase 1: HELLO Handshake
//! 1. Both sides send HELLO frame with: protocol version, auth role, offered rooms
//! 2. State machine validates and negotiates
//! 3. Compute intersection of offered rooms
//! 4. If intersection is empty, connection fails
//!
//! ### Phase 2: Room Communication
//! Once handshake completes:
//! 1. Send PublishRooms to SessionManager
//! 2. Route room messages bidirectionally
//! 3. Serialize outbound typed messages → bytes
//! 4. Deserialize inbound bytes → typed messages

pub mod actor;
pub mod auth;
pub mod connection_manager;
pub mod error;
pub mod handshake;
pub mod protocol;
pub mod serialize;
pub mod session_bridge;
pub mod session_messages;

#[cfg(test)]
mod integration_tests;
