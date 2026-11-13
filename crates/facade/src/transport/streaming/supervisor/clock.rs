//! One monotonic epoch for body acceptance and common delivery deadlines.

use std::time::Duration;

#[cfg(test)]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::HttpStreamingError;

pub(super) struct StreamClock {
    epoch: tokio::time::Instant,
    #[cfg(test)]
    manual: AtomicBool,
    #[cfg(test)]
    manual_now: AtomicU64,
}

impl StreamClock {
    pub(super) fn new() -> Self {
        Self {
            epoch: tokio::time::Instant::now(),
            #[cfg(test)]
            manual: AtomicBool::new(false),
            #[cfg(test)]
            manual_now: AtomicU64::new(0),
        }
    }

    pub(super) fn now(&self) -> Result<u64, HttpStreamingError> {
        #[cfg(test)]
        if self.manual.load(Ordering::Acquire) {
            return Ok(self.manual_now.load(Ordering::Acquire));
        }
        let elapsed = tokio::time::Instant::now()
            .checked_duration_since(self.epoch)
            .ok_or(HttpStreamingError::Clock)?;
        u64::try_from(elapsed.as_nanos()).map_err(|_| HttpStreamingError::Clock)
    }

    pub(super) fn deadline(&self, nanos: u64) -> Result<tokio::time::Instant, HttpStreamingError> {
        #[cfg(test)]
        if self.manual.load(Ordering::Acquire) {
            let remaining = nanos.saturating_sub(self.manual_now.load(Ordering::Acquire));
            return tokio::time::Instant::now()
                .checked_add(Duration::from_nanos(remaining))
                .ok_or(HttpStreamingError::Clock);
        }
        self.epoch
            .checked_add(Duration::from_nanos(nanos))
            .ok_or(HttpStreamingError::Clock)
    }

    #[cfg(test)]
    pub(super) fn set_for_test(&self, nanos: u64) -> Result<(), HttpStreamingError> {
        if self.manual.load(Ordering::Acquire) && nanos < self.manual_now.load(Ordering::Acquire) {
            return Err(HttpStreamingError::Clock);
        }
        self.manual_now.store(nanos, Ordering::Release);
        self.manual.store(true, Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_time_allows_equality_and_rejects_regression() {
        let clock = StreamClock::new();
        assert_eq!(clock.set_for_test(7), Ok(()));
        assert_eq!(clock.set_for_test(7), Ok(()));
        assert_eq!(clock.set_for_test(6), Err(HttpStreamingError::Clock));
        assert_eq!(clock.now(), Ok(7));
    }
}
