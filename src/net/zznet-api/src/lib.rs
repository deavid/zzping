//! # zznet-api
//!
//! Transport abstraction layer for the ZZPing network stack.
//!
//! This crate provides the abstract traits that define the transport layer interface,
//! enabling the rest of the network stack to be completely transport-agnostic.
//!

pub mod error;
pub mod mock;
/// Network layer traits for interface segregation (control-plane vs data-plane).
pub mod traits;
pub mod transport;
pub mod types;

// Re-export key traits for convenience
pub use traits::{MessageRouter, PeerRegistry};
