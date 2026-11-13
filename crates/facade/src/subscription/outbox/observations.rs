//! Fixed once-only observations of successful whole-event evictions.

use std::iter::FusedIterator;
use std::ops::RangeInclusive;

#[derive(Debug, Default)]
pub(super) struct DropObservationState {
    pending: Option<RangeInclusive<u64>>,
}

impl DropObservationState {
    pub(super) fn record_source_range(&mut self, first: u64, last: u64) {
        let first = self.pending.as_ref().map_or(first, |range| *range.start());
        // A source may run while the outside owner is idle. Coalesce large
        // jumps rather than turn a cumulative counter into unbounded log work.
        let first =
            if last.saturating_sub(first) >= crate::SubscriptionLimits::MAX_QUEUED_EVENTS as u64 {
                last
            } else {
                first
            };
        self.pending = Some(first..=last);
    }
    pub(super) fn record(&mut self, total: u64) {
        self.pending = Some(match self.pending.take() {
            Some(previous) => *previous.start()..=total,
            None => total..=total,
        });
    }

    pub(super) fn take(&mut self) -> SubscriptionDropTotals {
        SubscriptionDropTotals {
            pending: self.pending.take(),
        }
    }
}

/// Move-only successful cumulative drop totals from the common controller.
///
/// The range is contiguous because the controller advances its exact counter
/// once for each successful whole-event eviction. Taking it adds no queue,
/// allocation, callback, or protocol-specific inference.
#[must_use = "drop observations are consumed only by iterating them"]
pub(crate) struct SubscriptionDropTotals {
    pending: Option<RangeInclusive<u64>>,
}

impl Iterator for SubscriptionDropTotals {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        self.pending.as_mut().and_then(Iterator::next)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.pending
            .as_ref()
            .map_or((0, Some(0)), Iterator::size_hint)
    }
}

impl FusedIterator for SubscriptionDropTotals {}
