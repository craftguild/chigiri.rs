//! Combine two actual eviction stages once before immutable wire finalization.

#[cfg(any(test, feature = "subscription-worker"))]
use std::sync::Arc;

#[cfg(any(test, feature = "subscription-worker"))]
use chigiri_runtime::__private::HostSourceDelivery;
use chigiri_runtime::SubscriptionOverflow;

use super::{SubscriptionOutbox, SubscriptionOutboxSignal};

impl SubscriptionOutbox {
    #[cfg(any(test, feature = "subscription-worker"))]
    pub(crate) fn attach_source_delivery(&mut self, source: Option<Arc<HostSourceDelivery>>) {
        if let Some(source) = source {
            match &self.source_delivery {
                Some(previous) if !Arc::ptr_eq(previous, &source) => {
                    self.terminate(SubscriptionOutboxSignal::Invariant);
                }
                Some(_) => {}
                None => {
                    self.source_delivery = Some(source);
                }
            }
        }
    }

    pub(super) fn sync_source_drops(&mut self) -> Result<(), SubscriptionOutboxSignal> {
        if self.terminal_counter_frozen {
            return Ok(());
        }
        let total = match &self.source_delivery {
            Some(source) => source
                .snapshot()
                .ok_or(SubscriptionOutboxSignal::Accounting),
            None => return Ok(()),
        };
        total
            .and_then(|total| self.merge_source_total(total))
            .map_err(|signal| self.terminate(signal))
    }

    pub(super) fn freeze_source_drops(&mut self) -> Result<(), SubscriptionOutboxSignal> {
        if self.terminal_counter_frozen {
            return Ok(());
        }
        let total = match &self.source_delivery {
            Some(source) => source
                .freeze()
                .ok_or(SubscriptionOutboxSignal::Accounting)?,
            None => return Ok(()),
        };
        self.merge_source_total(total)
    }

    fn merge_source_total(&mut self, total: u64) -> Result<(), SubscriptionOutboxSignal> {
        let delta = total
            .checked_sub(self.source_dropped)
            .ok_or(SubscriptionOutboxSignal::Invariant)?;
        if delta == 0 {
            return Ok(());
        }
        if self.overflow != SubscriptionOverflow::DropOldest {
            return Err(SubscriptionOutboxSignal::Invariant);
        }
        let (before, after) = self
            .controller
            .as_mut()
            .ok_or(SubscriptionOutboxSignal::Accounting)?
            .include_source_drops(delta)
            .map_err(SubscriptionOutboxSignal::from_decision)?;
        self.source_dropped = total;
        #[cfg(any(test, feature = "subscription-worker"))]
        self.drop_observations
            .record_source_range(before + 1, after);
        #[cfg(not(any(test, feature = "subscription-worker")))]
        let _ = (before, after);
        Ok(())
    }
}

impl Drop for SubscriptionOutbox {
    fn drop(&mut self) {
        if let Some(source) = &self.source_delivery {
            source.freeze();
        }
    }
}
