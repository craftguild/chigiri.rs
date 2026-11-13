//! Existing common outbox integration without another event queue or task.

use crate::subscription::outbox::SubscriptionOutboxSignal;
use crate::subscription::worker::websocket::WebSocketTerminalCarrier;

use super::failure::{WebSocketOperationSharedFailure, is_shared_delivery};
use super::state::{Phase, WebSocketOperationSlot};

pub(super) enum EventCutoff {
    Retire,
    AlreadyRetired,
    Invalidate,
}

impl<C: Send + Sync + 'static> WebSocketOperationSlot<C> {
    pub(super) fn enter_terminal(
        &mut self,
        terminal: WebSocketTerminalCarrier,
        cutoff: EventCutoff,
    ) -> Result<(), WebSocketOperationSharedFailure> {
        if terminal.generation() != self.generation {
            return Err(invariant());
        }
        if let Some(events) = &mut self.events {
            events.freeze_terminal_counter();
            match cutoff {
                EventCutoff::Retire => events.retire(&terminal).map_err(delivery)?,
                EventCutoff::AlreadyRetired => {}
                EventCutoff::Invalidate => events.invalidate(),
            }
        }
        self.phase = Phase::Terminal(super::terminal::TerminalState::new(terminal));
        self.delivery.take();
        self.mark_released()?;
        Ok(())
    }

    pub(super) fn poll_event_timer(
        &mut self,
        now: u64,
    ) -> Result<(), WebSocketOperationSharedFailure> {
        let result = self
            .events
            .as_mut()
            .map_or(Ok(()), |events| match events.on_timer(now) {
                Err(SubscriptionOutboxSignal::Wait) => Ok(()),
                result => result,
            });
        self.handle_event_result(result).map(drop)
    }

    pub(super) fn handle_event_result<T>(
        &mut self,
        result: Result<T, SubscriptionOutboxSignal>,
    ) -> Result<Option<T>, WebSocketOperationSharedFailure> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(error) if is_shared_delivery(error) => {
                self.convert_delivery_failure(error)?;
                Err(delivery(error))
            }
            Err(SubscriptionOutboxSignal::Wait) => Ok(None),
            Err(error) => {
                self.convert_delivery_failure(error)?;
                Ok(None)
            }
        }
    }

    fn convert_delivery_failure(
        &mut self,
        signal: SubscriptionOutboxSignal,
    ) -> Result<(), WebSocketOperationSharedFailure> {
        let phase = std::mem::replace(&mut self.phase, Phase::Transitioning);
        match phase {
            Phase::Prepared { worker, .. } => {
                let terminal = worker.delivery_failure(signal)?;
                self.enter_terminal(terminal, EventCutoff::Invalidate)
            }
            Phase::Subscription { mut worker, .. } => {
                let outbox = self
                    .events
                    .as_mut()
                    .and_then(|events| events.observation_outbox());
                let terminal = worker.delivery_failure(signal, outbox)?;
                self.enter_terminal(terminal, EventCutoff::Invalidate)
            }
            Phase::Terminal(mut terminal) => {
                let response = self.failure.response(
                    crate::subscription::failure::outbox_failure_class(signal),
                    &signal,
                );
                terminal
                    .carrier_mut()
                    .replace_with_error(response)
                    .map_err(|()| invariant())?;
                if let Some(events) = &mut self.events {
                    events.invalidate();
                }
                self.phase = Phase::Terminal(terminal);
                Ok(())
            }
            other => {
                self.phase = other;
                Err(invariant())
            }
        }
    }

    pub(super) fn mark_released(&mut self) -> Result<(), WebSocketOperationSharedFailure> {
        if self.released {
            return Ok(());
        }
        self.release_id = Some(self.wire_id.as_ref().ok_or_else(invariant)?.clone());
        self.released = true;
        Ok(())
    }
}

pub(super) const fn delivery(error: SubscriptionOutboxSignal) -> WebSocketOperationSharedFailure {
    WebSocketOperationSharedFailure::Delivery(error)
}

fn invariant() -> WebSocketOperationSharedFailure {
    WebSocketOperationSharedFailure::Worker(
        crate::subscription::worker::SubscriptionWorkerRuntimeError::Invariant,
    )
}
