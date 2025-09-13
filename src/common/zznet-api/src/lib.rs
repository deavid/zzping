use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Represents the role of a participant in the ZZPing network protocol.
/// This enum defines the possible roles that can connect to the network.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Role {
    /// Collector role for gathering data.
    Collector,
    /// Database role for storing data.
    Database,
    /// Read-only client role.
    ClientRo,
    /// Administrative client role with full access.
    ClientAdmin,
}

/// The abstract interface for a bidirectional communication channel.
/// Application components will depend only on this trait.
#[async_trait]
pub trait ZzChannel: Send + Sync {
    /// Sends a payload of bytes over the channel.
    async fn send(&self, payload: Vec<u8>) -> Result<()>;
    /// Receives a payload of bytes from the channel.
    /// Returns `Ok(None)` if the channel has been closed.
    async fn recv(&mut self) -> Result<Option<Vec<u8>>>;
}
