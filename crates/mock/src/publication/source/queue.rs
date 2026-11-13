//! Apply inclusive count, retained-byte, age and shared-budget admission by OR.

use std::collections::VecDeque;
use std::sync::Arc;
use std::task::Waker;
use std::time::{Duration, Instant};

use chigiri::SubscriptionOverflow;

use super::accounting::{DeliveryLease, Usage};
use super::port::{Termination, failure};
use crate::events::{
    SourceLimits,
    retention::{Reservation, Retention},
};
use crate::luau::ScriptError;
use crate::publication::{CapturedEvent, MAX_SAFE_SEQUENCE, PublicationError};

pub(super) struct Entry {
    pub(super) event: CapturedEvent,
    at: Instant,
}

pub(super) struct Queue {
    pub(super) entries: VecDeque<Entry>,
    pub(super) terminal: Option<ScriptError>,
    pub(super) finished: bool,
    pub(super) closed: bool,
    pub(super) waker: Option<Waker>,
    pub(super) overflow: SubscriptionOverflow,
    pub(super) dropped: u64,
    pub(super) delivery: Arc<chigiri::__private::HostSourceDelivery>,
    pub(super) usage: Arc<Usage>,
    limits: SourceLimits,
    retention: Retention,
    _reservation: Reservation,
}

impl Queue {
    pub(super) fn new(
        limits: SourceLimits,
        retention: Retention,
    ) -> Result<Self, PublicationError> {
        let bytes = limits
            .max_queued_events()
            .checked_mul(size_of::<Entry>())
            .and_then(|bytes| {
                bytes.checked_add(size_of::<super::port::SourcePort>() + 2 * size_of::<usize>())
            })
            .ok_or(PublicationError::ArithmeticOverflow)?;
        let reservation = retention
            .reserve(bytes, usize::MAX)
            .map_err(|_| PublicationError::CaptureLimit)?;
        let counter_reservation = retention
            .reserve(
                chigiri::__private::HostSourceDelivery::allocation_bytes()
                    + size_of::<Reservation>()
                    + 2 * size_of::<usize>(),
                usize::MAX,
            )
            .map_err(|_| PublicationError::CaptureLimit)?;
        let mut entries = VecDeque::new();
        entries
            .try_reserve_exact(limits.max_queued_events())
            .map_err(|_| PublicationError::Allocation)?;
        Ok(Self {
            entries,
            terminal: None,
            finished: false,
            closed: false,
            waker: None,
            overflow: SubscriptionOverflow::Backpressure,
            dropped: 0,
            delivery: chigiri::__private::HostSourceDelivery::retained(Arc::new(
                counter_reservation,
            )),
            usage: Usage::new(&retention)?,
            limits,
            retention,
            _reservation: reservation,
        })
    }

    pub(super) fn terminate(&mut self, reason: Termination) {
        if !self.closed {
            self.closed = true;
            self.terminal = Some(failure(reason));
            self.delivery.freeze();
            self.entries.clear();
        }
    }

    fn evict(&mut self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        if self.dropped == MAX_SAFE_SEQUENCE {
            self.terminate(Termination::Counter);
            return false;
        }
        let Some(total) = self.delivery.record_drop() else {
            self.terminate(Termination::Shutdown);
            return false;
        };
        drop(self.entries.pop_front());
        self.dropped = total;
        true
    }

    pub(super) fn expire(&mut self, now: Instant) {
        if self.delivery.is_stopped() {
            self.terminate(Termination::Shutdown);
            return;
        }
        let age = Duration::from_millis(self.limits.max_queue_age_ms());
        while self
            .entries
            .front()
            .is_some_and(|entry| now.saturating_duration_since(entry.at) > age)
        {
            if self.overflow != SubscriptionOverflow::DropOldest {
                self.terminate(Termination::Age);
                break;
            }
            if !self.evict() {
                break;
            }
        }
    }

    pub(super) fn relieve_staging_pressure(&mut self) {
        if self.closed || self.usage.bytes() == 0 {
            return;
        }
        if self.overflow != SubscriptionOverflow::DropOldest || !self.evict() {
            self.terminate(Termination::Pressure);
        }
    }

    pub(super) fn offer(&mut self, mut event: CapturedEvent, now: Instant) {
        self.expire(now);
        if self.closed || self.finished {
            return;
        }
        loop {
            let fits = self.entries.len() < self.limits.max_queued_events()
                && self
                    .usage
                    .bytes()
                    .checked_add(event.source_bytes())
                    .is_some_and(|bytes| bytes <= self.limits.max_queued_bytes());
            let lease = fits.then(|| {
                DeliveryLease::reserve(
                    &self.usage,
                    &self.retention,
                    event.source_bytes(),
                    self.limits.max_queued_bytes(),
                )
            });
            if let Some(Ok(lease)) = lease {
                event.attach_delivery(lease);
                self.entries.push_back(Entry { event, at: now });
                return;
            }
            if self.overflow != SubscriptionOverflow::DropOldest || !self.evict() {
                self.terminate(Termination::Pressure);
                return;
            }
        }
    }
}
