//! A monotonic clock contract that decouples provider tests from wall time.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fmt, time::Instant};

use chigiri_rate::MonotonicTick;

/// Object-safe monotonic clock used consistently by a provider.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> MonotonicTick;
}

impl<C> Clock for Arc<C>
where
    C: Clock,
{
    fn now(&self) -> MonotonicTick {
        self.as_ref().now()
    }
}

/// Manually controlled clock for deterministic provider conformance tests.
#[derive(Default)]
pub struct ManualClock {
    now: AtomicU64,
}

impl fmt::Debug for ManualClock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManualClock")
            .finish_non_exhaustive()
    }
}

impl ManualClock {
    pub const fn new(now: MonotonicTick) -> Self {
        Self {
            now: AtomicU64::new(now.as_nanos()),
        }
    }

    /// Advances the clock by `nanos`, saturating at the maximum tick.
    pub fn advance(&self, nanos: u64) {
        let _ = self
            .now
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |now| {
                Some(now.saturating_add(nanos))
            });
    }

    /// Sets the clock to any tick, including an earlier one, for provider tests.
    pub fn set(&self, now: MonotonicTick) {
        self.now.store(now.as_nanos(), Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> MonotonicTick {
        MonotonicTick::from_nanos(self.now.load(Ordering::SeqCst))
    }
}

/// Production monotonic clock for process-local providers.
pub struct SystemClock {
    origin: Instant,
}

impl fmt::Debug for SystemClock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SystemClock")
            .finish_non_exhaustive()
    }
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> MonotonicTick {
        let nanos = self.origin.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        MonotonicTick::from_nanos(nanos)
    }
}
