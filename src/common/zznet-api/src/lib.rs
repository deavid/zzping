use anyhow::Result;
use async_trait::async_trait;

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
