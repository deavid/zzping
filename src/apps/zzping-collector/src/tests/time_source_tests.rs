use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::pinger::TimeSource;

/// A minimal mock TimeSource that counts resync() calls and provides
/// monotonically increasing timestamps.
struct MockTimeSource {
    resync_count: Arc<AtomicUsize>,
    last: u64,
}

impl MockTimeSource {
    fn new() -> Self {
        Self {
            resync_count: Arc::new(AtomicUsize::new(0)),
            // arbitrary starting ns
            last: 1_600_000_000_000_000_000u64,
        }
    }
    fn counter(&self) -> Arc<AtomicUsize> {
        self.resync_count.clone()
    }
}

impl TimeSource for MockTimeSource {
    fn now_ns(&mut self) -> Option<u64> {
        // advance by 1 microsecond each call
        self.last = self.last.saturating_add(1_000);
        Some(self.last)
    }
    fn resync(&mut self) {
        self.resync_count.fetch_add(1, Ordering::SeqCst);
    }
    fn last_generated_ns(&self) -> u64 {
        self.last
    }
}

#[test]
fn mock_time_source_resync_counter() {
    let mut ts = MockTimeSource::new();
    let counter = ts.counter();
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    ts.resync();
    ts.resync();
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}
