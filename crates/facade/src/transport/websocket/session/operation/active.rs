//! Active subscription demand and terminal cleanup polling.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::event::{EventCutoff, delivery};
use super::failure::{WebSocketOperationSharedFailure, is_shared_delivery};
use super::state::{Phase, WebSocketOperationSlot, WebSocketOperationTransition};
use super::terminal::TerminalState;
use crate::subscription::control::WebSocketControlStaging;
use crate::subscription::outbox::SubscriptionOutboxSignal;
use crate::subscription::worker::SubscriptionWorkerRuntimeError;
use crate::subscription::worker::websocket::{
    ActiveWebSocketSubscription, WebSocketSubscriptionEvent, WebSocketSubscriptionSetup,
    WebSocketSubscriptionSetupOutcome, WebSocketSubscriptionTermination,
};
use crate::transport::websocket::session::events::WebSocketEventOutbox;

impl<C: Send + Sync + 'static> WebSocketOperationSlot<C> {
    pub(super) fn poll_subscription_setup(
        &mut self,
        mut worker: WebSocketSubscriptionSetup,
        context: &mut Context<'_>,
    ) -> Result<Poll<WebSocketOperationTransition>, WebSocketOperationSharedFailure> {
        match Pin::new(&mut worker).poll(context) {
            Poll::Pending => {
                self.phase = Phase::SubscriptionSetup(worker);
                Ok(Poll::Pending)
            }
            Poll::Ready(WebSocketSubscriptionSetupOutcome::Ready {
                mut subscription,
                overflow,
            }) => {
                let mut outbox = match self.build_outbox(overflow) {
                    Ok(outbox) => outbox,
                    Err(WebSocketOperationSharedFailure::Delivery(signal))
                        if !is_shared_delivery(signal) =>
                    {
                        let terminal = subscription.delivery_failure(signal, None)?;
                        self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                        return Ok(Poll::Ready(self.take_transition()));
                    }
                    Err(error) => return Err(error),
                };
                outbox.attach_source_delivery(subscription.source_delivery());
                let physical = subscription.physical_lease()?;
                let emission = subscription.emission_lease()?;
                let events = match WebSocketEventOutbox::new(
                    outbox,
                    physical,
                    emission,
                    self.send_timeout()?,
                ) {
                    Ok(events) => events,
                    Err(signal) if !is_shared_delivery(signal) => {
                        let terminal = subscription.delivery_failure(signal, None)?;
                        self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                        return Ok(Poll::Ready(self.take_transition()));
                    }
                    Err(signal) => return Err(delivery(signal)),
                };
                self.events = Some(events);
                self.phase = Phase::Subscription {
                    worker: *subscription,
                    demand_outstanding: false,
                };
                Ok(Poll::Ready(WebSocketOperationTransition::Progress))
            }
            Poll::Ready(WebSocketSubscriptionSetupOutcome::Terminal(terminal)) => {
                let cutoff = if terminal.permits_prior_event_drain() {
                    EventCutoff::Retire
                } else {
                    EventCutoff::Invalidate
                };
                self.enter_terminal(terminal, cutoff)?;
                Ok(Poll::Ready(self.take_transition()))
            }
            Poll::Ready(WebSocketSubscriptionSetupOutcome::SharedFailure(error)) => {
                Err(error.into())
            }
        }
    }

    pub(super) fn poll_subscription(
        &mut self,
        mut worker: ActiveWebSocketSubscription,
        mut demand_outstanding: bool,
        context: &mut Context<'_>,
        now: u64,
    ) -> Result<Poll<WebSocketOperationTransition>, WebSocketOperationSharedFailure> {
        if self.draining && !self.graceful_event {
            return self.poll_graceful_subscription(worker, demand_outstanding);
        }
        let outbox = self
            .events
            .as_mut()
            .and_then(WebSocketEventOutbox::observation_outbox)
            .ok_or_else(invariant)?;
        match worker.poll_event(context, outbox) {
            Poll::Ready(Ok(WebSocketSubscriptionEvent::Event {
                reservation,
                response,
            })) => {
                if !demand_outstanding {
                    return Err(invariant());
                }
                demand_outstanding = false;
                let result = self
                    .events
                    .as_mut()
                    .ok_or_else(invariant)?
                    .encode_and_enqueue(reservation, &response, now);
                worker.activity.finish();
                if self.draining {
                    self.graceful_event = false;
                }
                let outbox = self
                    .events
                    .as_mut()
                    .and_then(WebSocketEventOutbox::observation_outbox)
                    .ok_or_else(invariant)?;
                worker.observe_drops(outbox);
                if let Err(signal) = result {
                    let terminal = worker.delivery_failure(signal, Some(outbox))?;
                    if is_shared_delivery(signal) {
                        self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                        return Err(delivery(signal));
                    }
                    self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                    return Ok(Poll::Ready(self.take_transition()));
                }
                self.phase = Phase::Subscription {
                    worker,
                    demand_outstanding,
                };
                return Ok(Poll::Ready(WebSocketOperationTransition::Progress));
            }
            Poll::Ready(Ok(WebSocketSubscriptionEvent::Terminal { terminal })) => {
                let cutoff = if terminal.permits_prior_event_drain() {
                    EventCutoff::Retire
                } else {
                    EventCutoff::Invalidate
                };
                self.enter_terminal(terminal, cutoff)?;
                return Ok(Poll::Ready(self.take_transition()));
            }
            Poll::Ready(Err(error)) => return Err(error.into()),
            Poll::Pending => {}
        }
        if self.draining || demand_outstanding {
            self.phase = Phase::Subscription {
                worker,
                demand_outstanding,
            };
            return Ok(Poll::Pending);
        }
        let result = self
            .events
            .as_mut()
            .ok_or_else(invariant)?
            .reserve_before_poll(now);
        let outbox = self
            .events
            .as_mut()
            .and_then(WebSocketEventOutbox::observation_outbox)
            .ok_or_else(invariant)?;
        worker.observe_drops(outbox);
        let reservation = match result {
            Ok(reservation) => reservation,
            Err(SubscriptionOutboxSignal::Wait) => {
                self.phase = Phase::Subscription {
                    worker,
                    demand_outstanding,
                };
                return Ok(Poll::Pending);
            }
            Err(signal) => {
                let terminal = worker.delivery_failure(signal, Some(outbox))?;
                self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                if is_shared_delivery(signal) {
                    return Err(delivery(signal));
                }
                return Ok(Poll::Ready(self.take_transition()));
            }
        };
        worker.request_event(reservation).map_err(|_| invariant())?;
        self.phase = Phase::Subscription {
            worker,
            demand_outstanding: true,
        };
        Ok(Poll::Ready(WebSocketOperationTransition::Progress))
    }

    pub(super) fn poll_termination(
        &mut self,
        mut worker: WebSocketSubscriptionTermination,
        context: &mut Context<'_>,
    ) -> Result<Poll<WebSocketOperationTransition>, WebSocketOperationSharedFailure> {
        match Pin::new(&mut worker).poll(context) {
            Poll::Pending => {
                self.phase = Phase::Terminating(worker);
                Ok(Poll::Pending)
            }
            Poll::Ready(Ok(terminal)) => {
                self.enter_terminal(terminal, EventCutoff::Invalidate)?;
                Ok(Poll::Ready(self.take_transition()))
            }
            Poll::Ready(Err(error)) => Err(error.into()),
        }
    }

    pub(super) fn poll_terminal(
        &mut self,
        mut terminal: TerminalState,
        context: &mut Context<'_>,
        controls: &mut WebSocketControlStaging,
    ) -> Result<Poll<WebSocketOperationTransition>, WebSocketOperationSharedFailure> {
        let cleanup_ready = terminal.poll_cleanup(context).is_ready();
        let drained = self
            .events
            .as_ref()
            .is_none_or(WebSocketEventOutbox::is_drained);
        let mut progressed = false;
        if drained && !terminal.wire_done() {
            let id = self.wire_id.as_ref().ok_or_else(invariant)?.clone();
            progressed = terminal.stage(id, controls)?;
            if terminal.wire_done() {
                self.wire_id.take();
            }
        }
        if cleanup_ready && drained && terminal.wire_done() {
            self.phase = Phase::Finished;
            self.finished = true;
            return Ok(Poll::Ready(WebSocketOperationTransition::Finished));
        }
        self.phase = Phase::Terminal(terminal);
        if progressed {
            Ok(Poll::Ready(WebSocketOperationTransition::Progress))
        } else {
            Ok(Poll::Pending)
        }
    }
}

fn invariant() -> WebSocketOperationSharedFailure {
    WebSocketOperationSharedFailure::Worker(SubscriptionWorkerRuntimeError::Invariant)
}
