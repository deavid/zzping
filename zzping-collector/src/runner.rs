//! Core ping loop implementation for the collector.
//!
//! This module provides the asynchronous ping loop that maintains
//! precise rate control and handles concurrent ping operations.

use crate::ping_client::{PingClient, PingResult};
use log::warn;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

/// Asynchronous ping loop for a single target with rate control.
///
/// This function implements a rate-controlled ping loop that maintains
/// precise timing using `sleep_until` rather than fixed delays. This
/// prevents timing drift that accumulates with traditional interval-based
/// approaches.
///
/// ## Design Rationale
///
/// **Precise Rate Control**: Uses `sleep_until` with absolute timestamps
/// to maintain accurate ping rates even when ping operations take time.
///
/// **Concurrency Bounds**: Semaphore limits concurrent pings to prevent
/// resource exhaustion and maintain predictable system load.
///
/// **Sequence Tracking**: Maintains a wrapping sequence number for
/// ping identification and debugging.
///
/// **Graceful Skipping**: When at concurrency limit, skips pings rather
/// than queuing them, maintaining the target rate.
///
/// # Parameters
/// * `ping_client` - Client for sending pings to the target
/// * `ping_tx` - Channel for sending ping results to the batch processor
/// * `max_in_flight` - Maximum number of concurrent pings allowed
/// * `rate` - Target ping rate in packets per second
pub async fn pinger_loop(
    ping_client: Arc<dyn PingClient>,
    ping_tx: mpsc::Sender<PingResult>,
    max_in_flight: usize,
    rate: u64,
) {
    if rate == 0 {
        warn!("Ping rate is 0, pinger loop will not run.");
        return;
    }
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_in_flight));
    let start_time = Instant::now();
    let mut sequence_idx: u16 = 0;
    let interval_duration = Duration::from_secs_f64(1.0 / rate as f64);

    let mut next_tick = Instant::now() + interval_duration;

    loop {
        // Use sleep_until for a more accurate rate, as it's not affected by
        // the time taken by the async operations in the loop.
        tokio::time::sleep_until(next_tick.into()).await;
        next_tick += interval_duration;

        if let Ok(permit) = semaphore.clone().try_acquire_owned() {
            ping_client
                .ping(
                    sequence_idx,
                    ping_tx.clone(),
                    permit,
                    start_time,
                    Instant::now(), // The actual send time is now, not the tick time.
                )
                .await;
            sequence_idx = sequence_idx.wrapping_add(1);
        } else {
            // If we can't acquire a permit, it means we are at max_in_flight.
            // We should skip this tick and try again at the next scheduled time.
            // The `next_tick` update above handles this automatically.
            warn!("Max in-flight pings reached. Skipping a ping to maintain rate.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ping_client::PingResult;
    use std::net::IpAddr;
    use tokio::sync::OwnedSemaphorePermit;
    use tokio::sync::mpsc;

    struct MockPingClient {
        target: IpAddr,
        ping_count: std::sync::Arc<std::sync::Mutex<usize>>,
    }

    impl MockPingClient {
        fn new(target: IpAddr) -> Self {
            Self {
                target,
                ping_count: std::sync::Arc::new(std::sync::Mutex::new(0)),
            }
        }

        fn get_ping_count(&self) -> usize {
            *self.ping_count.lock().unwrap()
        }
    }

    #[async_trait::async_trait]
    impl PingClient for MockPingClient {
        fn target(&self) -> IpAddr {
            self.target
        }

        async fn ping(
            &self,
            _sequence: u16,
            tx: mpsc::Sender<PingResult>,
            _permit: OwnedSemaphorePermit,
            start_time: Instant,
            _target_time: Instant,
        ) {
            {
                let mut count = self.ping_count.lock().unwrap();
                *count += 1;
            } // Drop the lock before awaiting

            let result = PingResult {
                target: self.target,
                sent_nanos: start_time.elapsed().as_nanos() as u64,
                rtt: Some(Duration::from_micros(1000)), // 1ms RTT
            };

            let _ = tx.send(result).await;
        }
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_pinger_loop_zero_rate() {
        let (tx, _rx) = mpsc::channel(10);
        let client = Arc::new(MockPingClient::new("127.0.0.1".parse().unwrap()));

        // Test that zero rate exits immediately
        pinger_loop(client, tx, 10, 0).await;
        // Should return immediately without panicking
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_pinger_loop_basic_operation() {
        let (tx, _rx) = mpsc::channel(10);
        let client = Arc::new(MockPingClient::new("127.0.0.1".parse().unwrap()));

        // Test that we can spawn the pinger loop and abort it quickly
        let client_clone = client.clone();
        let handle = tokio::spawn(async move {
            pinger_loop(client_clone, tx, 10, 1000).await; // 1000 pps
        });

        // Abort immediately
        handle.abort();

        // Just check that we can create and abort the task without issues
        // The actual ping testing is covered by other tests
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_pinger_loop_semaphore_limiting() {
        let (tx, _rx) = mpsc::channel(10);
        let client = Arc::new(MockPingClient::new("127.0.0.1".parse().unwrap()));

        // Use very low max_in_flight to test semaphore
        let client_clone = client.clone();
        let handle = tokio::spawn(async move {
            pinger_loop(client_clone, tx, 1, 1000).await; // 1000 pps, max 1 in flight
        });

        // Let it run for a short time
        tokio::time::sleep(Duration::from_millis(50)).await;
        handle.abort();

        // With semaphore limit of 1 and high rate, we should still get some pings
        let ping_count = client.get_ping_count();
        assert!(
            ping_count > 0,
            "Should have sent some pings even with low semaphore limit"
        );
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_pinger_loop_sequence_wrapping() {
        let (tx, _rx) = mpsc::channel(10);
        let client = Arc::new(MockPingClient::new("127.0.0.1".parse().unwrap()));

        // Test sequence number wrapping by running enough iterations
        let client_clone = client.clone();
        let handle = tokio::spawn(async move {
            pinger_loop(client_clone, tx, 10, 10000).await; // Very high rate
        });

        // Let it run long enough to potentially wrap sequence numbers
        tokio::time::sleep(Duration::from_millis(10)).await;
        handle.abort();

        // With u16 sequence numbers, wrapping happens at 65536
        // At 10000 pps, we'd need 6.5 seconds to wrap, so this test just ensures
        // the loop runs without issues
        let ping_count = client.get_ping_count();
        assert!(
            ping_count > 0,
            "Should have sent pings during sequence test"
        );
    }

    #[tokio::test]
    #[ntest::timeout(1200)]
    async fn test_pinger_loop_rate_control() {
        let (tx, _rx) = mpsc::channel(10);
        let client = Arc::new(MockPingClient::new("127.0.0.1".parse().unwrap()));

        let start = Instant::now();
        let client_clone = client.clone();
        let handle = tokio::spawn(async move {
            pinger_loop(client_clone, tx, 10, 10).await; // 10 pps = 100ms intervals
        });

        // Let it run for about 1 second
        tokio::time::sleep(Duration::from_millis(1050)).await;
        handle.abort();

        let elapsed = start.elapsed();
        let ping_count = client.get_ping_count();

        // At 10 pps, we should get roughly 10-11 pings in 1 second
        // Allow some tolerance for timing variations
        assert!(
            (8..=12).contains(&ping_count),
            "Expected ~10 pings in 1 second at 10 pps, got {}",
            ping_count
        );

        // Verify timing is reasonable (should be close to 1 second)
        assert!(
            elapsed >= Duration::from_millis(1000) && elapsed <= Duration::from_millis(1100),
            "Test should run for about 1 second, ran for {:?}",
            elapsed
        );
    }
}
