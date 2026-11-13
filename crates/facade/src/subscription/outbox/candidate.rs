//! One bounded encoded candidate retains its exact original charge until admission.

use chigiri_http::EncodedSubscriptionEvent;

use super::{SubscriptionEventReservation, SubscriptionOutbox, SubscriptionOutboxSignal};
use crate::subscription::delivery::{DeliveryDecision, MonotonicInstant};

pub(super) struct EncodedCandidate {
    reservation: SubscriptionEventReservation,
    encoded: EncodedSubscriptionEvent,
}

impl EncodedCandidate {
    pub(super) const fn new(
        reservation: SubscriptionEventReservation,
        encoded: EncodedSubscriptionEvent,
    ) -> Self {
        Self {
            reservation,
            encoded,
        }
    }
}

impl SubscriptionOutbox {
    pub(super) fn admit_candidate(
        &mut self,
        now: MonotonicInstant,
    ) -> Result<(), SubscriptionOutboxSignal> {
        let Some(candidate) = &self.candidate else {
            return Ok(());
        };
        let actual_bytes = candidate.encoded.len();
        loop {
            match self.controller_ref()?.evaluate_candidate(now, actual_bytes) {
                DeliveryDecision::Admit => break,
                DeliveryDecision::EvictWhole(count) => self.evict(count)?,
                // Only a non-evictable physical event can prevent recovery.
                // Retain this one capped candidate and stop further demand.
                DeliveryDecision::Pause => return Ok(()),
                decision => return Err(self.terminate_decision(decision)),
            }
        }
        let candidate = self.candidate.take().expect("one encoded candidate");
        self.controller_mut()?
            .enqueue(
                candidate.reservation.inner,
                actual_bytes,
                candidate.encoded,
                now,
            )
            .map_err(|decision| self.terminate_decision(decision))
    }
}
