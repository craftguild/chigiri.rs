//! Immediate same-generation cancellation with actively polled cleanup.

use super::event::EventCutoff;
use super::failure::WebSocketOperationSharedFailure;
use super::state::{Phase, WebSocketOperationSlot};
use crate::subscription::control::WebSocketControlStaging;
use crate::subscription::worker::SubscriptionWorkerRuntimeError;

impl<C: Send + Sync + 'static> WebSocketOperationSlot<C> {
    pub(super) fn cancel_inner(
        &mut self,
        controls: &mut WebSocketControlStaging,
        decision: chigiri_runtime::__private::SubscriptionTermination,
    ) -> Result<(), WebSocketOperationSharedFailure> {
        controls.discard_generation(self.generation)?;
        if let Some(events) = &mut self.events {
            events.freeze_terminal_counter();
            events.invalidate();
        }
        let phase = std::mem::replace(&mut self.phase, Phase::Transitioning);
        match phase {
            Phase::Validation(mut worker) => {
                worker.cancel();
                self.phase = Phase::Validation(worker);
            }
            Phase::Prepared { worker, .. } => {
                let terminal = worker.cancel()?;
                self.enter_terminal(terminal, EventCutoff::Invalidate)?;
            }
            Phase::OneShot { mut worker, outbox } => {
                worker.cancel();
                self.phase = Phase::OneShot { worker, outbox };
            }
            Phase::SubscriptionSetup(worker) => {
                self.phase = Phase::Terminating(worker.cancel(decision));
            }
            Phase::Subscription { worker, .. } => {
                let outbox = self
                    .events
                    .as_mut()
                    .and_then(|events| events.observation_outbox());
                self.phase = Phase::Terminating(worker.cancel(decision, outbox));
            }
            Phase::Terminating(worker) => self.phase = Phase::Terminating(worker),
            Phase::Terminal(mut terminal) => {
                terminal.suppress();
                self.phase = Phase::Terminal(terminal);
            }
            Phase::Transitioning => return Err(invariant()),
            Phase::Finished => {
                self.phase = Phase::Finished;
                return Ok(());
            }
        }
        self.mark_released()?;
        self.wire_id.take();
        Ok(())
    }
}

fn invariant() -> WebSocketOperationSharedFailure {
    WebSocketOperationSharedFailure::Worker(SubscriptionWorkerRuntimeError::Invariant)
}
