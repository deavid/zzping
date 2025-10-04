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
//! ```rust,ignore
//! // Note: This is a conceptual example. See integration_tests.rs for working examples.
//! use zznet_session::session_manager::SessionManager;
//! use zznet_session::peer_session::PeerSession;
//! use zznet_session::types::{PeerId, RoomId};
//! use tokio::sync::mpsc;
//!
//! // Define your application message enum (must implement RoomMessageTrait)
//! enum MyAppMessages {
//!     MemDB(String),
//! }
//!
//! // Create two SessionManagers (simulating two processes)
//! let mut manager_a = SessionManager::<MyAppMessages>::new();
//! let mut manager_b = SessionManager::<MyAppMessages>::new();
//!
//! // Add peers with pre-constructed PeerSession
//! let peer_a = PeerSession::new(PeerId::from("peer_a"));
//! let peer_b = PeerSession::new(PeerId::from("peer_b"));
//!
//! manager_a.add_peer(PeerId::from("peer_b"), peer_b).unwrap();
//! manager_b.add_peer(PeerId::from("peer_a"), peer_a).unwrap();
//!
//! // Connect with channels (NO SERIALIZATION)
//! let (tx_a, rx_b) = mpsc::channel(10);
//! let (tx_b, rx_a) = mpsc::channel(10);
//!
//! manager_a.connect_peer(PeerId::from("peer_b"), tx_a, rx_a).unwrap();
//! manager_b.connect_peer(PeerId::from("peer_a"), tx_b, rx_b).unwrap();
//!
//! // Messages flow as typed structs, never serialized
//! # }
//! ```
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

pub mod peer_session;
pub mod room_adapter;
pub mod room_message_trait;
pub mod session_manager;
pub mod types;

#[cfg(test)]
pub mod test_room_messages;

#[cfg(test)]
mod integration_tests;
