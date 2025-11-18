//! Mock implementation of PingerClient for testing.

use async_trait::async_trait;
use std::net::IpAddr;
use std::time::Duration;

use crate::traits::{PingerClient, PingError};

/// Mock pinger client that simulates ping responses for testing.
#[derive(Debug, Clone)]
pub struct MockPingerClient {
    /// Whether pings should succeed or fail
    pub should_succeed: bool,
    /// Simulated round-trip time for successful pings
    pub rtt: Duration,
}

impl Default for MockPingerClient {
    fn default() -> Self {
        Self {
            should_succeed: true,
            rtt: Duration::from_millis(10),
        }
    }
}

impl MockPingerClient {
    /// Create a new mock pinger that succeeds with the given RTT.
    pub fn new_succeeding(rtt: Duration) -> Self {
        Self {
            should_succeed: true,
            rtt,
        }
    }

    /// Create a new mock pinger that always times out.
    pub fn new_timeout() -> Self {
        Self {
            should_succeed: false,
            rtt: Duration::from_millis(10), // Not used when failing
        }
    }
}

#[async_trait]
impl PingerClient for MockPingerClient {
    async fn ping(&self, _target: IpAddr, _seq: u16) -> Result<Duration, PingError> {
        if self.should_succeed {
            Ok(self.rtt)
        } else {
            Err(PingError::Timeout)
        }
    }
}