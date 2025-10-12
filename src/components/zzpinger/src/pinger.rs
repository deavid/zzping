//! Core ping logic for the Pinger component.
//!
//! Implements ICMP ping operations with timeout handling and sequence management.
//! Uses backend abstraction to enable testing without real network operations.

use futures::future::BoxFuture;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use surge_ping::{Client, Config, PingIdentifier, PingSequence};
use tokio::time::timeout;

use zzmem_db::network_messages::PingResult;

/// Abstraction for ping backends. Enables testing by allowing mock implementations.
/// Decouples ping logic from network operations for reliable, fast unit tests.
pub trait PingBackend: Send + Sync + 'static {
    /// Performs a ping operation asynchronously. Returns RTT in microseconds or None on failure.
    /// Uses BoxFuture to avoid lifetime complexity in trait objects.
    fn ping<'a>(
        &'a self,
        target: &'a str,
        sequence: u32,
        timeout_ms: u64,
    ) -> BoxFuture<'a, Option<u32>>;
}

/// Mock backend for testing. Returns deterministic RTT values without network calls.
/// Ensures tests are fast, reliable, and don't require special privileges or network access.
#[derive(Clone)]
pub struct MockBackend {
    pub next_rtt_us: Option<u32>,
}

impl MockBackend {
    pub fn new(next_rtt_us: Option<u32>) -> Self {
        Self { next_rtt_us }
    }
}

impl PingBackend for MockBackend {
    fn ping<'a>(
        &'a self,
        _target: &'a str,
        _sequence: u32,
        _timeout_ms: u64,
    ) -> BoxFuture<'a, Option<u32>> {
        let v = self.next_rtt_us;
        Box::pin(async move { v })
    }
}

/// Real ICMP ping backend using surge-ping. Performs actual network operations.
/// Requires CAP_NET_RAW privileges and network connectivity for successful pings.
pub struct RealPingBackend {}

impl RealPingBackend {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for RealPingBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl PingBackend for RealPingBackend {
    fn ping<'a>(
        &'a self,
        target: &'a str,
        sequence: u32,
        timeout_ms: u64,
    ) -> BoxFuture<'a, Option<u32>> {
        let target = target.to_string();
        Box::pin(async move {
            let timeout_duration = Duration::from_millis(timeout_ms);

            let config = Config::default();

            let client = match Client::new(&config) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("surge-ping client creation failed: {:?}", e);
                    return None;
                }
            };

            let addr = match target.parse() {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!("failed to parse target addr for ping: {} ({:?})", target, e);
                    return None;
                }
            };

            let mut pinger = client.pinger(addr, PingIdentifier(sequence as u16)).await;
            pinger.timeout(timeout_duration);

            let ping_future = pinger.ping(PingSequence(sequence as u16), &[]);

            match timeout(timeout_duration, ping_future).await {
                Ok(Ok((_, duration))) => Some(duration.as_micros() as u32),
                _ => None,
            }
        })
    }
}

/// Manages ping operations for a single target. Tracks sequence and timing.
/// Uses injected backend for testability, ensuring no real ICMP in unit tests.
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
    /// Ping backend implementation (real or mock)
    backend: Arc<dyn PingBackend>,
}

impl std::fmt::Debug for TargetPinger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TargetPinger")
            .field("target", &self.target)
            .field("rate_ms", &self.rate_ms)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

/// Creates a new TargetPinger with default MockBackend. Safe for tests as it doesn't perform real ICMP.
/// Use new_with_backend for production or custom backends.
impl TargetPinger {
    /// Convenience constructor that uses a MockBackend (safe for tests)
    pub fn new(target: String, rate_ms: u64, timeout_ms: u64) -> Self {
        Self::new_with_backend(
            target,
            rate_ms,
            timeout_ms,
            Arc::new(MockBackend::new(None)),
        )
    }

    /// Create a new TargetPinger for the given target configuration using the default (mockable) backend
    pub fn new_with_backend(
        target: String,
        rate_ms: u64,
        timeout_ms: u64,
        backend: Arc<dyn PingBackend>,
    ) -> Self {
        Self {
            target,
            rate_ms,
            timeout_ms,
            sequence: AtomicU32::new(0),
            last_ping_ms: AtomicU64::new(0),
            backend,
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

        // Use injected backend to perform ping
        let result = self
            .backend
            .ping(&self.target, sequence, self.timeout_ms)
            .await;

        PingResult {
            target: self.target.clone(),
            timestamp_ms,
            rtt_us: result,
            sequence,
        }
    }
}

impl Clone for TargetPinger {
    fn clone(&self) -> Self {
        // Copy atomic values into new atomics and clone backend
        let seq = self.sequence.load(Ordering::Relaxed);
        let last = self.last_ping_ms.load(Ordering::Relaxed);
        Self {
            target: self.target.clone(),
            rate_ms: self.rate_ms,
            timeout_ms: self.timeout_ms,
            sequence: AtomicU32::new(seq),
            last_ping_ms: AtomicU64::new(last),
            backend: Arc::clone(&self.backend),
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
