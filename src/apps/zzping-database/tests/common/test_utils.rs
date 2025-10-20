//! Test utilities for E2E tests.
//!
//! Provides helpers for:
//! - Tracing initialization and log capture
//! - Time advancement and virtual time control
//! - Configuration creation
//! - Verification helpers

use std::time::Duration;
use tracing_subscriber::EnvFilter;

/// Initialize comprehensive tracing for tests.
///
/// Configures detailed logging with:
/// - Test writer (captured by test framework)
/// - DEBUG level by default
/// - TRACE for protocol layers
/// - Target names and line numbers
///
/// # Returns
/// Guard that must be held for duration of test.
/// Tracing is reset when guard is dropped.
///
/// # Example
/// ```no_run
/// #[tokio::test]
/// async fn my_test() {
///     let _guard = init_test_tracing();
///     // Test runs with tracing enabled
/// }
/// ```
#[allow(dead_code)] // Function is actually used in one test but clippy flags it. The reason is that each test is a different compilation target.
pub fn init_test_tracing() -> tracing::subscriber::DefaultGuard {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into())
                .add_directive("zznet_hello=trace".parse().unwrap())
                .add_directive("zznet_session=debug".parse().unwrap())
                .add_directive("zzintent_config=debug".parse().unwrap())
                .add_directive("zzpinger=debug".parse().unwrap())
                .add_directive("zzmem_db=debug".parse().unwrap())
                .add_directive("zzcollector_state=debug".parse().unwrap())
                .add_directive("zzping_collector=debug".parse().unwrap())
                .add_directive("zzping_database=debug".parse().unwrap()),
        )
        .with_test_writer()
        .with_target(true)
        .with_line_number(true)
        .finish();

    tracing::subscriber::set_default(subscriber)
}

/// Advance virtual time and yield to let actors process.
///
/// When using `tokio::time::pause()`, this function:
/// 1. Advances the virtual clock by `duration`
/// 2. Yields to allow pending tasks to run
/// 3. Sleeps briefly (virtual time) to let actors settle
///
/// This ensures that:
/// - All scheduled timers are triggered
/// - Actor mailboxes are processed
/// - Cross-thread message delivery completes
///
/// # Example
/// ```no_run
/// #[tokio::test]
/// async fn my_test() {
///     let _guard = init_test_tracing();
///     tokio::time::pause();
///
///     // ... start actors ...
///
///     advance_time_and_yield(Duration::from_millis(150)).await;
///     // Now actors have processed 150ms of events
/// }
/// ```
pub async fn advance_time_and_yield(duration: Duration) {
    tokio::time::advance(duration).await;
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(10)).await;
}

/// Wait for a condition with timeout.
///
/// Repeatedly checks the condition until it returns true or timeout is reached.
/// Useful for verifying eventually-consistent state after time advancement.
///
/// # Panics
/// Panics if condition is not met before timeout.
///
/// # Example
/// ```no_run
/// #[tokio::test]
/// async fn my_test() {
///     let _guard = init_test_tracing();
///     let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
///     let counter_clone = counter.clone();
///
///     wait_for(
///         move || counter_clone.load(std::sync::atomic::Ordering::Relaxed) >= 5,
///         Duration::from_secs(5),
///         "counter reaches 5"
///     ).await;
/// }
/// ```
pub async fn wait_for<F>(condition: F, timeout: Duration, description: &str)
where
    F: Fn() -> bool,
{
    let start = tokio::time::Instant::now();
    let mut checks = 0u64;

    while !condition() {
        if start.elapsed() > timeout {
            panic!(
                "Timeout ({}ms) waiting for: {} (checked {} times)",
                timeout.as_millis(),
                description,
                checks
            );
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        checks += 1;
    }

    tracing::debug!(
        checks,
        elapsed_ms = start.elapsed().as_millis(),
        "Condition met: {}",
        description
    );
}

/// Wait for a condition with immediate first check.
///
/// Like `wait_for`, but checks the condition synchronously first,
/// making it useful for conditions that should already be true.
///
/// # Example
/// ```no_run
/// #[tokio::test]
/// async fn my_test() {
///     let _guard = init_test_tracing();
///     let value = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
///
///     // Simulate some work that updates value
///     advance_time_and_yield(Duration::from_millis(100)).await;
///
///     wait_for_immediate(
///         || value.load(std::sync::atomic::Ordering::Relaxed) == 1,
///         Duration::from_secs(1),
///         "value becomes 1"
///     ).await;
/// }
/// ```
pub async fn wait_for_immediate<F>(condition: F, timeout: Duration, description: &str)
where
    F: Fn() -> bool,
{
    if condition() {
        tracing::debug!("Condition already true: {}", description);
        return;
    }
    wait_for(condition, timeout, description).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_advance_time_and_yield() {
        tokio::time::pause();

        let start = tokio::time::Instant::now();
        advance_time_and_yield(Duration::from_millis(500)).await;
        let elapsed = start.elapsed();

        // Should have advanced by approximately 500ms
        assert!(
            elapsed >= Duration::from_millis(500),
            "Time not advanced properly: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_wait_for_success() {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let task = tokio::spawn(async move {
            for i in 1..=5 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                counter_clone.store(i, std::sync::atomic::Ordering::Relaxed);
            }
        });

        wait_for(
            || counter.load(std::sync::atomic::Ordering::Relaxed) >= 5,
            Duration::from_secs(5),
            "counter reaches 5",
        )
        .await;

        task.await.unwrap();
    }

    #[tokio::test]
    #[should_panic(expected = "Timeout")]
    async fn test_wait_for_timeout() {
        wait_for(
            || false, // Always false
            Duration::from_millis(50),
            "impossible condition",
        )
        .await;
    }

    #[tokio::test]
    async fn test_wait_for_immediate_already_true() {
        let value = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));

        // Should return immediately without waiting
        wait_for_immediate(
            || value.load(std::sync::atomic::Ordering::Relaxed),
            Duration::from_secs(5),
            "value is true",
        )
        .await;
    }
}
