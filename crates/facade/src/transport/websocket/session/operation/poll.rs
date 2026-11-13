//! Inline polling of validation, active execution, delivery, and cleanup.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri_runtime::{__private::WebSocketOperationKind, SubscriptionOverflow};

use super::event::{EventCutoff, delivery};
use super::failure::{WebSocketOperationSharedFailure, is_shared_delivery};
use super::state::{Phase, WebSocketOperationSlot, WebSocketOperationTransition};
use crate::subscription::control::WebSocketControlStaging;
use crate::subscription::outbox::{SubscriptionOutbox, SubscriptionOutboxSignal};
use crate::subscription::worker::SubscriptionWorkerRuntimeError;
use crate::subscription::worker::websocket::{WebSocketOneShotOutcome, WebSocketValidationOutcome};
use crate::transport::websocket::session::events::{
    WebSocketEventOutbox, WebSocketOneShotEventFailure,
};

impl<C: Send + Sync + 'static> WebSocketOperationSlot<C> {
    pub(in crate::transport::websocket::session) fn poll(
        &mut self,
        context: &mut Context<'_>,
        now: u64,
        controls: &mut WebSocketControlStaging,
    ) -> Poll<Result<WebSocketOperationTransition, WebSocketOperationSharedFailure>> {
        if self.finished {
            return Poll::Ready(Ok(WebSocketOperationTransition::Finished));
        }
        if self.release_id.is_some() {
            return Poll::Ready(Ok(self.take_transition()));
        }
        self.poll_event_timer(now)?;
        let phase = std::mem::replace(&mut self.phase, Phase::Transitioning);
        let transition = match phase {
            Phase::Validation(worker) => self.poll_validation(worker, context),
            Phase::Prepared { worker, outbox } => self.poll_prepared(*worker, outbox, context, now),
            Phase::OneShot { worker, outbox } => self.poll_one_shot(*worker, outbox, context, now),
            Phase::SubscriptionSetup(worker) => self.poll_subscription_setup(worker, context),
            Phase::Subscription {
                worker,
                demand_outstanding,
            } => self.poll_subscription(worker, demand_outstanding, context, now),
            Phase::Terminating(worker) => self.poll_termination(worker, context),
            Phase::Terminal(terminal) => self.poll_terminal(terminal, context, controls),
            Phase::Transitioning => Err(invariant()),
            Phase::Finished => {
                self.phase = Phase::Finished;
                self.finished = true;
                Ok(Poll::Ready(WebSocketOperationTransition::Finished))
            }
        }?;
        transition.map(Ok)
    }

    pub(super) fn take_transition(&mut self) -> WebSocketOperationTransition {
        if let Some(id) = self.release_id.take() {
            return WebSocketOperationTransition::Released {
                id,
                generation: self.generation,
            };
        }
        if self.finished {
            WebSocketOperationTransition::Finished
        } else {
            WebSocketOperationTransition::Progress
        }
    }

    fn poll_validation(
        &mut self,
        mut worker: crate::subscription::worker::websocket::WebSocketValidationWorker<C>,
        context: &mut Context<'_>,
    ) -> Result<Poll<WebSocketOperationTransition>, WebSocketOperationSharedFailure> {
        match Pin::new(&mut worker).poll(context) {
            Poll::Pending => {
                self.phase = Phase::Validation(worker);
                Ok(Poll::Pending)
            }
            Poll::Ready(WebSocketValidationOutcome::Prepared(worker)) => {
                self.phase = Phase::Prepared {
                    worker,
                    outbox: None,
                };
                Ok(Poll::Ready(WebSocketOperationTransition::Progress))
            }
            Poll::Ready(WebSocketValidationOutcome::Terminal(terminal)) => {
                self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                Ok(Poll::Ready(self.take_transition()))
            }
            Poll::Ready(WebSocketValidationOutcome::SharedFailure(error)) => Err(error.into()),
        }
    }

    fn poll_prepared(
        &mut self,
        mut worker: crate::subscription::worker::websocket::PreparedWebSocketWorkerOperation<C>,
        mut outbox: Option<SubscriptionOutbox>,
        context: &mut Context<'_>,
        now: u64,
    ) -> Result<Poll<WebSocketOperationTransition>, WebSocketOperationSharedFailure> {
        match worker.poll_setup_expired(context) {
            Poll::Ready(result) => {
                let terminal = worker.setup_failure(result)?;
                self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                return Ok(Poll::Ready(self.take_transition()));
            }
            Poll::Pending => {}
        }
        if let Some(delivery_outbox) = &mut outbox {
            match delivery_outbox.on_timer(now) {
                Ok(()) | Err(SubscriptionOutboxSignal::Wait) => {}
                Err(error) if is_shared_delivery(error) => return Err(delivery(error)),
                Err(error) => {
                    let terminal = worker.delivery_failure(error)?;
                    self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                    return Ok(Poll::Ready(self.take_transition()));
                }
            }
        }
        match worker.kind() {
            WebSocketOperationKind::Subscription => match worker.activate_subscription() {
                Ok(setup) => {
                    self.phase = Phase::SubscriptionSetup(setup);
                    Ok(Poll::Ready(WebSocketOperationTransition::Progress))
                }
                Err(failure) => {
                    let terminal = failure.into_terminal()?;
                    self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                    Ok(Poll::Ready(self.take_transition()))
                }
            },
            WebSocketOperationKind::Query | WebSocketOperationKind::Mutation => {
                if outbox.is_none() {
                    match self.build_outbox(SubscriptionOverflow::Backpressure) {
                        Ok(created) => outbox = Some(created),
                        Err(WebSocketOperationSharedFailure::Delivery(error))
                            if !is_shared_delivery(error) =>
                        {
                            let terminal = worker.delivery_failure(error)?;
                            self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                            return Ok(Poll::Ready(self.take_transition()));
                        }
                        Err(error) => return Err(error),
                    }
                }
                let reservation = match outbox
                    .as_mut()
                    .expect("one-shot outbox")
                    .reserve_before_poll(now)
                {
                    Ok(reservation) => reservation,
                    Err(SubscriptionOutboxSignal::Wait) => {
                        self.phase = Phase::Prepared {
                            worker: Box::new(worker),
                            outbox,
                        };
                        return Ok(Poll::Pending);
                    }
                    Err(error) if is_shared_delivery(error) => return Err(delivery(error)),
                    Err(error) => {
                        let terminal = worker.delivery_failure(error)?;
                        self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                        return Ok(Poll::Ready(self.take_transition()));
                    }
                };
                match worker.activate_one_shot(reservation) {
                    Ok(active) => {
                        self.finite = true;
                        self.phase = Phase::OneShot {
                            worker: Box::new(active),
                            outbox: outbox.expect("one-shot outbox"),
                        };
                        Ok(Poll::Ready(WebSocketOperationTransition::Progress))
                    }
                    Err(failure) => {
                        let terminal = failure.into_terminal()?;
                        self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                        Ok(Poll::Ready(self.take_transition()))
                    }
                }
            }
        }
    }

    fn poll_one_shot(
        &mut self,
        mut worker: crate::subscription::worker::websocket::ActiveWebSocketOneShot,
        outbox: SubscriptionOutbox,
        context: &mut Context<'_>,
        now: u64,
    ) -> Result<Poll<WebSocketOperationTransition>, WebSocketOperationSharedFailure> {
        match Pin::new(&mut worker).poll(context) {
            Poll::Pending => {
                self.phase = Phase::OneShot {
                    worker: Box::new(worker),
                    outbox,
                };
                Ok(Poll::Pending)
            }
            Poll::Ready(WebSocketOneShotOutcome::Ready(joined)) => {
                match WebSocketEventOutbox::from_one_shot(outbox, joined, self.send_timeout()?, now)
                {
                    Ok((events, terminal)) => {
                        self.events = Some(events);
                        self.enter_terminal(terminal, EventCutoff::AlreadyRetired)?;
                    }
                    Err(WebSocketOneShotEventFailure::Operation { signal, terminal }) => {
                        if is_shared_delivery(signal) {
                            return Err(delivery(signal));
                        }
                        let response = self.failure.response(
                            crate::subscription::failure::outbox_failure_class(signal),
                            &signal,
                        );
                        let mut terminal = *terminal;
                        terminal
                            .replace_with_error(response)
                            .map_err(|()| invariant())?;
                        self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                    }
                    Err(WebSocketOneShotEventFailure::SharedInvariant(error)) => {
                        return Err(error.into());
                    }
                }
                Ok(Poll::Ready(self.take_transition()))
            }
            Poll::Ready(WebSocketOneShotOutcome::Terminal(terminal)) => {
                self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                Ok(Poll::Ready(self.take_transition()))
            }
            Poll::Ready(WebSocketOneShotOutcome::SharedFailure(error)) => Err(error.into()),
        }
    }

    pub(super) fn build_outbox(
        &self,
        overflow: SubscriptionOverflow,
    ) -> Result<SubscriptionOutbox, WebSocketOperationSharedFailure> {
        let result = self.delivery.as_ref().ok_or_else(invariant)?.build(
            overflow,
            self.wire_id.as_ref().ok_or_else(invariant)?.clone(),
        );
        result.map_err(delivery)
    }

    pub(super) fn send_timeout(
        &self,
    ) -> Result<std::time::Duration, WebSocketOperationSharedFailure> {
        self.delivery
            .as_ref()
            .map(super::delivery::WebSocketOperationDeliverySeed::send_timeout)
            .ok_or_else(invariant)
    }
}

fn invariant() -> WebSocketOperationSharedFailure {
    WebSocketOperationSharedFailure::Worker(SubscriptionWorkerRuntimeError::Invariant)
}
