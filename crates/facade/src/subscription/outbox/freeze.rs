//! Logical terminal freeze while already admitted frames finish bounded delivery.

use super::{SubscriptionOutbox, SubscriptionOutboxSignal};

impl SubscriptionOutbox {
    /// Discards backlog, retaining the immutable current send and its charge.
    /// Returns whether sending a newer active result would skip an older item.
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn discard_waiting(&mut self) -> bool {
        let candidate = self.candidate.take();
        let queue = self
            .controller
            .as_mut()
            .map(|controller| controller.discard_unstarted());
        let skipped = candidate.is_some() || queue.as_ref().is_some_and(|queue| !queue.is_empty());
        drop(candidate);
        drop(queue);
        skipped
    }

    /// Freezes and returns the exact terminal delivery counter idempotently.
    ///
    /// This discards only a not-yet-queued candidate. Already queued and
    /// in-flight frames retain their existing ownership and deadlines.
    #[cfg(any(test, feature = "subscription-worker"))]
    pub(crate) fn freeze_terminal_counter(&mut self) -> u64 {
        if !self.terminal_counter_frozen {
            let _ = self.freeze_source_drops();
            self.candidate.take();
            if let Some(controller) = &mut self.controller {
                self.last_dropped_total = controller.freeze_evictions();
            }
            self.terminal_counter_frozen = true;
        }
        self.last_dropped_total
    }

    pub(super) fn require_source_demand(&mut self) -> Result<(), SubscriptionOutboxSignal> {
        self.check_terminal()?;
        if self.terminal_counter_frozen {
            Err(SubscriptionOutboxSignal::Cancelled)
        } else {
            Ok(())
        }
    }
}
