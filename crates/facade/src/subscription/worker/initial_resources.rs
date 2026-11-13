//! Move-only initial-response resources admitted before the validation worker.

use std::sync::{Arc, Mutex};

use chigiri_http::ResponseParts;

use crate::subscription::control::{
    InitialCompletionFallbackCharge, InitialCompletionFallbacks, InitialPreparationByteGuards,
    InitialResponseBufferGuard, SubscriptionControlSignal,
};
use crate::subscription::delivery::EventByteCharge;

struct InitialResponseBufferSlot {
    guard: Mutex<Option<InitialResponseBufferGuard>>,
}

/// Sole outer capability for converting the maximum reservation to exact bytes.
pub(crate) struct InitialResponseBufferLease {
    slot: Arc<InitialResponseBufferSlot>,
    #[cfg(test)]
    shrink_failures: usize,
}

impl InitialResponseBufferLease {
    #[cfg(test)]
    pub(crate) fn finish(
        mut self,
        response: &ResponseParts,
    ) -> Result<EventByteCharge, SubscriptionControlSignal> {
        self.try_finish(response)
    }

    pub(crate) fn try_finish(
        &mut self,
        response: &ResponseParts,
    ) -> Result<EventByteCharge, SubscriptionControlSignal> {
        #[cfg(test)]
        if self.shrink_failures > 0 {
            self.shrink_failures -= 1;
            return Err(SubscriptionControlSignal::Accounting);
        }
        let mut guard = self
            .slot
            .guard
            .lock()
            .map_err(|_| SubscriptionControlSignal::Accounting)?;
        let charge = guard
            .as_mut()
            .ok_or(SubscriptionControlSignal::Accounting)?
            .try_finish(response)?;
        guard.take();
        Ok(charge)
    }

    pub(crate) fn retains_maximum(&self) -> bool {
        self.slot.guard.lock().is_ok_and(|guard| {
            guard
                .as_ref()
                .is_some_and(InitialResponseBufferGuard::retains_maximum)
        })
    }

    #[cfg(test)]
    pub(crate) fn fail_shrinks(&mut self, count: usize) {
        self.shrink_failures = count;
    }
}

/// One atomic pre-worker admission bundle carried across both worker phases.
pub(crate) struct InitialWorkerResources {
    task_inputs: Option<InitialCompletionFallbacks>,
    fallback_charge: Option<InitialCompletionFallbackCharge>,
    response_buffer: Option<Arc<InitialResponseBufferSlot>>,
    response_lease_issued: bool,
}

impl InitialWorkerResources {
    pub(crate) fn new(
        task_inputs: InitialCompletionFallbacks,
        guards: InitialPreparationByteGuards,
    ) -> Self {
        let (fallback_charge, response_buffer) = guards.into_parts();
        Self {
            task_inputs: Some(task_inputs),
            fallback_charge: Some(fallback_charge),
            response_buffer: Some(Arc::new(InitialResponseBufferSlot {
                guard: Mutex::new(Some(response_buffer)),
            })),
            response_lease_issued: false,
        }
    }

    /// Moves only uncharged values into the task; physical owners remain outside.
    pub(crate) fn take_task_inputs(&mut self) -> Option<InitialCompletionFallbacks> {
        self.task_inputs.take()
    }

    /// Issues the sole outer lease while retaining the guard in stable cleanup storage.
    pub(crate) fn take_response_buffer(&mut self) -> Option<InitialResponseBufferLease> {
        if self.response_lease_issued {
            return None;
        }
        let slot = Arc::clone(self.response_buffer.as_ref()?);
        self.response_lease_issued = true;
        Some(InitialResponseBufferLease {
            slot,
            #[cfg(test)]
            shrink_failures: 0,
        })
    }

    /// Releases callback candidates after the task publishes its sole result.
    pub(crate) fn release_fallbacks(&mut self) {
        self.task_inputs.take();
        self.fallback_charge.take();
    }

    /// A finite pre-source response needs only the maximum wire reservation.
    pub(crate) fn into_finite_buffer(mut self) -> Option<InitialResponseBufferLease> {
        self.task_inputs.take();
        self.fallback_charge.take();
        self.take_response_buffer()
    }
}
