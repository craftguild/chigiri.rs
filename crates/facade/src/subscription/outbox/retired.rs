//! Progress-only supervision of a physical event after source termination.

use std::sync::Arc;
use std::time::Duration;

use super::SubscriptionOutboxSignal;
use super::frame::SharedClock;
use crate::subscription::delivery::{InFlightProgress, MonotonicInstant};

/// No payload or charge is retained here; the original physical frame owns it.
pub(crate) struct RetiredSendProgress {
    inner: InFlightProgress,
    clock: Arc<SharedClock>,
    timeout: Duration,
}

impl RetiredSendProgress {
    pub(super) const fn new(
        inner: InFlightProgress,
        clock: Arc<SharedClock>,
        timeout: Duration,
    ) -> Self {
        Self {
            inner,
            clock,
            timeout,
        }
    }

    pub(crate) fn accept(
        &mut self,
        length: usize,
        now: u64,
    ) -> Result<(), SubscriptionOutboxSignal> {
        self.observe(now)?;
        self.inner
            .accept(length, MonotonicInstant::from_nanos(now))
            .map(|_| ())
            .map_err(|_| SubscriptionOutboxSignal::Accounting)
    }

    pub(crate) fn next_deadline(&self) -> Result<Option<u64>, SubscriptionOutboxSignal> {
        self.inner
            .no_progress_deadline(self.timeout)
            .map(|deadline| deadline.map(MonotonicInstant::as_nanos))
            .map_err(|_| SubscriptionOutboxSignal::Accounting)
    }

    pub(crate) fn on_timer(&self, now: u64) -> Result<(), SubscriptionOutboxSignal> {
        self.observe(now)?;
        if self
            .next_deadline()?
            .is_some_and(|deadline| now >= deadline)
        {
            return Err(SubscriptionOutboxSignal::SendStalled);
        }
        Ok(())
    }

    fn observe(&self, now: u64) -> Result<(), SubscriptionOutboxSignal> {
        if self.clock.observe(now) {
            Ok(())
        } else {
            Err(SubscriptionOutboxSignal::NonMonotonicTime)
        }
    }
}
