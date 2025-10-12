//! Core ping logic for the Pinger component.
//!
//! Implements ICMP ping operations with timeout handling and sequence management.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use surge_ping::{Client, Config, PingIdentifier, PingSequence};
use tokio::time::timeout;

use zzmem_db::network_messages::PingResult;

/// Manages ping operations for a single target
#[derive(Debug)]
pub struct TargetPinger {
    /// Target hostname or IP address
    target: String,
    /// Rate in milliseconds between pings
    rate_ms: u64,
    /// Timeout in milliseconds for ping responses
    timeout_ms: u64,
    /// Sequence number for this target (increments per ping)
    sequence: AtomicU32,
    /// Timestamp of last ping sent (milliseconds since epoch)
    last_ping_ms: AtomicU64,
}

impl TargetPinger {
    /// Create a new TargetPinger for the given target configuration
    pub fn new(target: String, rate_ms: u64, timeout_ms: u64) -> Self {
        Self {
            target,
            rate_ms,
            timeout_ms,
            sequence: AtomicU32::new(0),
            last_ping_ms: AtomicU64::new(0),
        }
    }

    /// Get the target address
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Get the ping rate in milliseconds
    pub fn rate_ms(&self) -> u64 {
        self.rate_ms
    }

    /// Get the timeout in milliseconds
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// Perform a single ping operation
    pub async fn ping(&self) -> PingResult {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let timestamp_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Update last ping timestamp
        self.last_ping_ms.store(timestamp_ms, Ordering::Relaxed);

        // Perform the actual ping
        let result = self.ping_once(sequence).await;

        PingResult {
            target: self.target.clone(),
            timestamp_ms,
            rtt_us: result,
            sequence,
        }
    }

    /// Perform the actual ICMP ping with timeout
    async fn ping_once(&self, sequence: u32) -> Option<u32> {
        let timeout_duration = Duration::from_millis(self.timeout_ms);

        // Create ping client
        let client = match Client::new(&Config::default()) {
            Ok(client) => client,
            Err(_) => return None, // Failed to create client
        };

        // Parse target address
        let addr = match self.target.parse() {
            Ok(addr) => addr,
            Err(_) => return None, // Invalid address
        };

        // Create pinger
        let mut pinger = client.pinger(addr, PingIdentifier(sequence as u16)).await;

        // Set timeout
        pinger.timeout(timeout_duration);

        // Perform ping with timeout
        let ping_future = pinger.ping(PingSequence(sequence as u16), &[]);

        match timeout(timeout_duration, ping_future).await {
            Ok(Ok((_, duration))) => {
                // Success - convert duration to microseconds
                Some(duration.as_micros() as u32)
            }
            _ => {
                // Timeout or error
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_target_pinger_creation() {
        let pinger = TargetPinger::new("8.8.8.8".to_string(), 1000, 5000);

        assert_eq!(pinger.target(), "8.8.8.8");
        assert_eq!(pinger.rate_ms(), 1000);
        assert_eq!(pinger.timeout_ms(), 5000);
    }

    #[tokio::test]
    async fn test_ping_result_structure() {
        let pinger = TargetPinger::new("127.0.0.1".to_string(), 1000, 100);

        // This will likely timeout for 127.0.0.1 with such a short timeout,
        // but we can test the structure
        let result = pinger.ping().await;

        assert_eq!(result.target, "127.0.0.1");
        assert!(result.timestamp_ms > 0);
        assert_eq!(result.sequence, 0); // First ping
    }

    #[tokio::test]
    async fn test_sequence_increment() {
        let pinger = TargetPinger::new("127.0.0.1".to_string(), 1000, 100);

        let result1 = pinger.ping().await;
        let result2 = pinger.ping().await;

        assert_eq!(result1.sequence, 0);
        assert_eq!(result2.sequence, 1);
    }
}
