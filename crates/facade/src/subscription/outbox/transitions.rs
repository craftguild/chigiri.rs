//! Keep terminal, timer and controller access transitions in one owner.

use chigiri_http::SubscriptionEventEncodingError;

use super::state::EventController;
use super::{SubscriptionOutbox, SubscriptionOutboxSignal};
use crate::subscription::delivery::{DeliveryDecision, MonotonicInstant};

impl SubscriptionOutbox {
    pub(super) fn observe_time(
        &mut self,
        now_ns: u64,
    ) -> Result<MonotonicInstant, SubscriptionOutboxSignal> {
        self.check_terminal()?;
        self.sync_source_drops()?;
        if !self.clock.observe(now_ns) {
            return Err(self.terminate(SubscriptionOutboxSignal::NonMonotonicTime));
        }
        Ok(MonotonicInstant::from_nanos(now_ns))
    }

    pub(super) fn check_terminal(&mut self) -> Result<(), SubscriptionOutboxSignal> {
        let signal = self
            .terminal
            .get()
            .unwrap_or(Some(SubscriptionOutboxSignal::Invariant));
        if let Some(signal) = signal {
            let _ = self.freeze_source_drops();
            self.terminal_counter_frozen = true;
            if let Some(controller) = &self.controller {
                self.last_dropped_total = controller.dropped_total();
            }
            self.candidate.take();
            self.controller.take();
            return Err(signal);
        }
        Ok(())
    }

    pub(super) fn controller_ref(&mut self) -> Result<&EventController, SubscriptionOutboxSignal> {
        self.check_terminal()?;
        self.controller
            .as_ref()
            .ok_or(SubscriptionOutboxSignal::Cancelled)
    }

    pub(super) fn controller_mut(
        &mut self,
    ) -> Result<&mut EventController, SubscriptionOutboxSignal> {
        self.check_terminal()?;
        self.controller
            .as_mut()
            .ok_or(SubscriptionOutboxSignal::Cancelled)
    }

    pub(super) fn terminate_decision(
        &mut self,
        decision: DeliveryDecision,
    ) -> SubscriptionOutboxSignal {
        self.terminate(SubscriptionOutboxSignal::from_decision(decision))
    }

    pub(super) fn terminate_encoding(
        &mut self,
        error: SubscriptionEventEncodingError,
    ) -> SubscriptionOutboxSignal {
        self.terminate(SubscriptionOutboxSignal::from_encoding(error))
    }

    pub(super) fn terminate(
        &mut self,
        signal: SubscriptionOutboxSignal,
    ) -> SubscriptionOutboxSignal {
        if signal == SubscriptionOutboxSignal::Wait {
            return signal;
        }
        let signal = self.terminal.set(signal);
        let _ = self.freeze_source_drops();
        self.terminal_counter_frozen = true;
        if let Some(controller) = &self.controller {
            self.last_dropped_total = controller.dropped_total();
        }
        self.candidate.take();
        self.controller.take();
        signal
    }
}
