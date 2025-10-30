//! # zznet-api
//!
//! Transport abstraction layer for the ZZPing network stack.
//!
//! This crate provides the abstract traits that define the transport layer interface,
//! enabling the rest of the network stack to be completely transport-agnostic.
//!

pub mod error;
pub mod mock;
pub mod transport;
pub mod types;
