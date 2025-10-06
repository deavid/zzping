//! # zznet-api
//!
//! Transport abstraction layer for the ZZPing network stack.
//!
//! This crate provides the abstract traits that define the transport layer interface,
//! enabling the rest of the network stack to be completely transport-agnostic.
//!
//! ## Architecture
//!
//! The transport layer sits at the bottom of the network stack:
//! - **Above**: HELLO handler (zznet-hello) handles protocol and serialization
//! - **Below**: Concrete implementations (TCP/TLS, mock, etc.)
//!
//! ## Key Traits
//!
//! - `TransportConnection`: A single bidirectional connection (bytes in/out)
//! - `TransportServer`: Accepts incoming connections
//! - `TransportClient`: Creates outgoing connections
//!
//! ## Mock Transport
//!
//! This crate includes a production-quality mock transport implementation
//! for testing without any network I/O. This validates that the abstraction
//! is truly transport-agnostic.

pub mod error;
pub mod mock;
pub mod transport;
pub mod types;

// Legacy re-exports for compatibility during migration
// TODO: Remove after migration complete
pub use types::Role;

// DEPRECATED: Old channel-based API
// Will be removed after migration to new transport architecture
use anyhow::Result;
use async_trait::async_trait;

/// DEPRECATED: Use transport::TransportConnection instead.
///
/// Backwards-compatibility shim for legacy channel-based code.
#[deprecated(
    since = "0.2.0",
    note = "Use transport::TransportConnection trait instead"
)]
#[async_trait]
pub trait ZzChannel: Send + Sync + std::fmt::Debug {
    /// Send raw bytes on the legacy channel.
    async fn send(&self, payload: Vec<u8>) -> Result<()>;

    /// Receive raw bytes from the legacy channel.
    async fn recv(&mut self) -> Result<Option<Vec<u8>>>;
}
