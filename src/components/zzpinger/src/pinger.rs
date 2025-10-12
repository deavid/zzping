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
    /// The next RTT (round-trip time) in microseconds that this mock will return.
    ///
    /// - `Some(u32)`: the mock `ping` call will immediately return this RTT value.
    /// - `None`: the mock `ping` call will simulate a timeout/failure and return `None`.
    ///
    /// This field enables deterministic unit tests by controlling the backend's response.
    pub next_rtt_us: Option<u32>,
}

impl MockBackend {
    /// Create a new `MockBackend` that will return `next_rtt_us` for each ping.
    ///
    /// Use `Some(value)` to simulate a successful ping with the given RTT (microseconds),
    /// or `None` to simulate failures/timeouts.
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
    /// Create a new `RealPingBackend` using the default surge-ping configuration.
    ///
    /// This backend performs real ICMP operations and therefore requires appropriate
    /// privileges (e.g. CAP_NET_RAW) and network connectivity when used.
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

/// Manages ping operations for a single target, tracking sequence numbers and timing.
///
/// Encapsulates all state for one ping target, including its configuration and the backend used for sending pings.
/// This design allows each target to be managed independently in its own asynchronous task.
/// By using an injectable `PingBackend`, it ensures that no real ICMP operations are performed during unit tests,
/// making tests fast, reliable, and free of special privilege requirements.
pub struct TargetPinger {
    target: String,
    rate_ms: u64,
    timeout_ms: u64,
    sequence: AtomicU32,
    last_ping_ms: AtomicU64,
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

impl TargetPinger {
    /// Creates a new pinger with a `MockBackend` for safe testing.
    ///
    /// This constructor is ideal for unit tests where real network operations are undesirable.
    /// It guarantees that no actual ICMP packets will be sent, preventing test flakiness and the need for root privileges.
    pub fn new(target: String, rate_ms: u64, timeout_ms: u64) -> Self {
        Self::new_with_backend(
            target,
            rate_ms,
            timeout_ms,
            Arc::new(MockBackend::new(None)),
        )
    }

    /// Creates a new pinger with a specified backend.
    ///
    /// This is the primary constructor for production use (with `RealPingBackend`) or for injecting
    /// custom mock backends in advanced testing scenarios. It allows decoupling the pinging logic
    /// from the underlying network implementation.
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

    /// Returns the network target (hostname or IP address) for this pinger.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Returns the configured rate in milliseconds at which pings are sent.
    /// This value determines the delay between consecutive ping operations in the ping loop.
    pub fn rate_ms(&self) -> u64 {
        self.rate_ms
    }

    /// Returns the configured timeout in milliseconds for awaiting a ping response.
    /// If a response is not received within this duration, the ping is considered lost.
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// Performs a single ping, returning the result.
    ///
    /// This method increments the sequence number, records the current timestamp, and uses the configured
    /// backend to send a ping. It's the core operation executed repeatedly by the pinging task.
    /// The returned `PingResult` contains all information about the outcome of this specific operation.
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

    #[test]
    fn test_real_ping_backend_default() {
        let _backend = RealPingBackend::default();
        // This test just ensures the default constructor can be called without panicking.
    }

    #[test]
    fn test_target_pinger_debug_format() {
        let pinger = TargetPinger::new("8.8.8.8".to_string(), 1000, 5000);
        let debug_str = format!("{:?}", pinger);
        assert!(debug_str.contains("TargetPinger"));
        assert!(debug_str.contains(r#"target: "8.8.8.8""#));
        assert!(debug_str.contains("rate_ms: 1000"));
        assert!(debug_str.contains("timeout_ms: 5000"));
    }
}
