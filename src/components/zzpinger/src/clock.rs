use std::sync::Arc;
use std::time::SystemTime;

use crate::traits::Clock;

/// A clock that aligns SystemTime to `tokio::time::Instant`, so tests that
/// pause/advance tokio time get deterministic SystemTime values.
#[derive(Clone)]
pub struct TokioAlignedClock {
    start_system: SystemTime,
    start_instant: tokio::time::Instant,
}

impl Default for TokioAlignedClock {
    fn default() -> Self {
        Self::new()
    }
}

impl TokioAlignedClock {
    /// Create a new tokio-aligned clock capturing the current SystemTime and
    /// tokio Instant. `now()` returns `start_system + (Instant::now() - start_instant)`.
    pub fn new() -> Self {
        Self {
            start_system: SystemTime::now(),
            start_instant: tokio::time::Instant::now(),
        }
    }

    /// Convenience constructor returning an Arc (common usage pattern)
    pub fn new_arc() -> Arc<Self> {
        Arc::new(Self::new())
    }
}

impl Clock for TokioAlignedClock {
    fn now(&self) -> SystemTime {
        let elapsed = tokio::time::Instant::now().duration_since(self.start_instant);
        self.start_system + elapsed
    }
}
