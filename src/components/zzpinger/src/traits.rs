//! Traits for the zzpinger component.

use async_trait::async_trait;
use std::net::IpAddr;
use std::time::Duration;

/// Error type for ping operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PingError {
    /// Ping timed out.
    Timeout,
    /// Network error occurred.
    NetworkError,
}

/// Trait for ping client implementations.
#[async_trait]
pub trait PingerClient: Send + Sync + 'static {
    /// Performs a ping to the target and returns the round-trip time.
    async fn ping(&self, target: IpAddr, seq: u16) -> Result<Duration, PingError>;
}

/// Trait for time source.
pub trait Clock: Send + Sync + 'static {
    /// Returns the current system time.
    fn now(&self) -> std::time::SystemTime;
}

/// Default system clock.
#[derive(Clone, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> std::time::SystemTime {
        std::time::SystemTime::now()
    }
}
