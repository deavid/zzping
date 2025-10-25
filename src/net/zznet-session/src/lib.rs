//! # zznet-session
//!
//! Proof of Concept #1.5: Minimal SessionManager for connection lifecycle testing
//!
//! This crate provides `SessionManager` which manages multiple peer sessions
//! and routes typed messages to rooms. It enables testing of connection
//! lifecycle scenarios (connect/disconnect/reconnect) without any network I/O.
//!
//! ## CRITICAL: 100% Typed, Zero Serialization
//!
//! This PoC operates ENTIRELY on typed Rust messages. There is:
//! - ❌ NO serialization (no serde, bincode, messagepack)
//! - ❌ NO bytes (no Vec<u8> anywhere)
//! - ❌ NO transport (no TCP, TLS, sockets)
//! - ❌ NO HELLO protocol
//! - ❌ NO framing, headers, or message boundaries
//! - ✅ ONLY typed Rust structs via mpsc channels
//!
//! ## Core Concept
//!
//! `SessionManager` manages multiple peer connections, where each peer
//! has a set of rooms. Messages are routed to the appropriate room
//! based on peer_id and room_id.
//!
//! ## Example
//!
//! ## Testing Connection Lifecycle
//!
//! This PoC enables testing:
//! - Late connections (peer added but not connected immediately)
//! - Mid-disconnections (peer disconnects during operation)
//! - Reconnections (peer reconnects after disconnect)
//! - Multiple peers simultaneously
//!
//! ## What's Deferred
//!
//! This PoC does NOT include:
//! - PublishRooms negotiation (PoC #2)
//! - Room intersection logic (PoC #2)
//! - Auto-join mechanism (PoC #2)
//! - Serialization layer (PoC #3)
//! - Transport layer (PoC #4)
//! - HELLO protocol (PoC #4)

/// Per-peer session state and helpers.
pub mod peer_session;

/// Adapter utilities for routing room messages into sessions.
pub mod room_adapter;

/// Traits for serializing/deserializing room messages.
pub mod room_message_trait;

/// Manages lifetime and routing for multiple peer sessions.
pub mod session_manager;

/// Actix message types for SessionManager actor pattern
pub mod messages;

// Export public types
pub use session_manager::SessionManager;

/// Common types used by session manager and peers.
pub mod types;

#[cfg(test)]
pub mod test_room_messages;

#[cfg(test)]
mod integration_tests;

#[cfg(test)]
mod actor_tests;
