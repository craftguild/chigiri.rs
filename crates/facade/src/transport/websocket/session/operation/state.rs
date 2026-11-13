//! Move-only operation phases and exact-generation public session seams.

use std::sync::Arc;
use std::task::{Context, Poll};

use crate::subscription::bridge_resources::BridgeGeneration;
use crate::subscription::control::{SubscriptionControlFrame, WebSocketControlStaging};
use crate::subscription::failure::SubscriptionFailureReporter;
use crate::subscription::outbox::SubscriptionOutbox;
use crate::subscription::worker::websocket::{
    ActiveWebSocketOneShot, ActiveWebSocketSubscription, PreparedWebSocketWorkerOperation,
    WebSocketSubscriptionSetup, WebSocketSubscriptionTermination, WebSocketTerminalCarrier,
    WebSocketValidationWorker,
};

use super::delivery::WebSocketOperationDeliverySeed;
use super::failure::WebSocketOperationSharedFailure;
use super::terminal::TerminalState;
use crate::transport::websocket::session::WebSocketWireFrame;
use crate::transport::websocket::session::events::WebSocketEventOutbox;

pub(in crate::transport::websocket::session) enum WebSocketOperationTransition {
    Progress,
    Released {
        id: Arc<str>,
        generation: BridgeGeneration,
    },
    Finished,
}

pub(in crate::transport::websocket::session) struct WebSocketOperationSlot<C: Send + Sync + 'static>
{
    pub(super) wire_id: Option<Arc<str>>,
    pub(super) generation: BridgeGeneration,
    pub(super) phase: Phase<C>,
    pub(super) delivery: Option<WebSocketOperationDeliverySeed>,
    pub(super) failure: SubscriptionFailureReporter,
    pub(super) events: Option<WebSocketEventOutbox>,
    pub(super) released: bool,
    pub(super) release_id: Option<Arc<str>>,
    pub(super) finished: bool,
    pub(super) draining: bool,
    pub(super) graceful_event: bool,
    pub(super) finite: bool,
}

pub(super) enum Phase<C: Send + Sync + 'static> {
    Validation(WebSocketValidationWorker<C>),
    Prepared {
        worker: Box<PreparedWebSocketWorkerOperation<C>>,
        outbox: Option<SubscriptionOutbox>,
    },
    OneShot {
        worker: Box<ActiveWebSocketOneShot>,
        outbox: SubscriptionOutbox,
    },
    SubscriptionSetup(WebSocketSubscriptionSetup),
    Subscription {
        worker: ActiveWebSocketSubscription,
        demand_outstanding: bool,
    },
    Terminating(WebSocketSubscriptionTermination),
    Terminal(TerminalState),
    Transitioning,
    Finished,
}

impl<C: Send + Sync + 'static> WebSocketOperationSlot<C> {
    pub(in crate::transport::websocket::session) fn new(
        id: Arc<str>,
        generation: BridgeGeneration,
        worker: WebSocketValidationWorker<C>,
        delivery: WebSocketOperationDeliverySeed,
        failure: SubscriptionFailureReporter,
    ) -> Result<Self, WebSocketOperationSharedFailure> {
        if worker.generation() != Some(generation) {
            return Err(WebSocketOperationSharedFailure::Worker(
                crate::subscription::worker::SubscriptionWorkerRuntimeError::Invariant,
            ));
        }
        Ok(Self {
            wire_id: Some(id),
            generation,
            phase: Phase::Validation(worker),
            delivery: Some(delivery),
            failure,
            events: None,
            released: false,
            release_id: None,
            finished: false,
            draining: false,
            graceful_event: false,
            finite: false,
        })
    }

    pub(in crate::transport::websocket::session) fn from_terminal(
        id: Arc<str>,
        terminal: WebSocketTerminalCarrier,
        delivery: WebSocketOperationDeliverySeed,
        failure: SubscriptionFailureReporter,
    ) -> Self {
        let generation = terminal.generation();
        Self {
            wire_id: Some(id.clone()),
            generation,
            phase: Phase::Terminal(TerminalState::new(terminal)),
            delivery: Some(delivery),
            failure,
            events: None,
            released: true,
            release_id: Some(id.clone()),
            finished: false,
            draining: false,
            graceful_event: false,
            finite: false,
        }
    }

    pub(in crate::transport::websocket::session) fn wire_id(&self) -> Option<&str> {
        self.wire_id.as_deref()
    }

    pub(in crate::transport::websocket::session) const fn generation(&self) -> BridgeGeneration {
        self.generation
    }

    pub(in crate::transport::websocket::session) fn live_generation(
        &self,
    ) -> Option<BridgeGeneration> {
        (!self.released && self.wire_id.is_some() && !self.finished).then_some(self.generation)
    }

    #[cfg(test)]
    pub(in crate::transport::websocket::session) const fn is_finished(&self) -> bool {
        self.finished
    }

    pub(in crate::transport::websocket::session) fn has_event(&self) -> bool {
        self.events
            .as_ref()
            .is_some_and(WebSocketEventOutbox::has_event)
    }

    pub(in crate::transport::websocket::session) fn begin_event(
        &mut self,
        now: u64,
    ) -> Result<Option<WebSocketWireFrame>, WebSocketOperationSharedFailure> {
        self.poll_event_timer(now)?;
        let result = self
            .events
            .as_mut()
            .map_or(Ok(None), |events| events.begin_send(now));
        match result {
            Err(crate::subscription::outbox::SubscriptionOutboxSignal::Wait) => Ok(None),
            result => self.handle_event_result(result).map(Option::flatten),
        }
    }

    pub(in crate::transport::websocket::session) fn record_event_progress(
        &mut self,
        accepted: usize,
        flushed: bool,
        now: u64,
    ) -> Result<(), WebSocketOperationSharedFailure> {
        let result = self
            .events
            .as_mut()
            .ok_or_else(invariant)?
            .record_progress(accepted, flushed, now);
        match result {
            Err(crate::subscription::outbox::SubscriptionOutboxSignal::Wait) => Ok(()),
            result => self.handle_event_result(result).map(drop),
        }
    }

    pub(in crate::transport::websocket::session) fn poll_finish_event(
        &mut self,
        context: &mut Context<'_>,
        now: u64,
    ) -> Poll<Result<bool, WebSocketOperationSharedFailure>> {
        let result = match self.events.as_mut() {
            Some(events) => events.poll_finish_send(context, now),
            None => return Poll::Ready(Ok(false)),
        };
        match result {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(crate::subscription::outbox::SubscriptionOutboxSignal::Wait)) => {
                Poll::Ready(Ok(false))
            }
            Poll::Ready(result) => Poll::Ready(
                self.handle_event_result(result)
                    .map(|value| value.unwrap_or(false)),
            ),
        }
    }

    pub(in crate::transport::websocket::session) fn next_deadline(
        &mut self,
    ) -> Result<Option<u64>, WebSocketOperationSharedFailure> {
        let result = match &mut self.phase {
            Phase::Prepared {
                outbox: Some(outbox),
                ..
            } => outbox.next_deadline(),
            _ => self
                .events
                .as_mut()
                .map_or(Ok(None), WebSocketEventOutbox::next_deadline),
        };
        match result {
            Err(crate::subscription::outbox::SubscriptionOutboxSignal::Wait) => Ok(None),
            result => self.handle_event_result(result).map(Option::flatten),
        }
    }

    pub(in crate::transport::websocket::session) fn authorize_terminal_control(
        &mut self,
        frame: &SubscriptionControlFrame,
    ) -> bool {
        let Phase::Terminal(terminal) = &mut self.phase else {
            return false;
        };
        terminal.authorize(frame, self.generation)
    }

    pub(in crate::transport::websocket::session) fn cancel(
        &mut self,
        controls: &mut WebSocketControlStaging,
    ) -> Result<WebSocketOperationTransition, WebSocketOperationSharedFailure> {
        self.cancel_inner(
            controls,
            chigiri_runtime::__private::SubscriptionTermination::ClientCancelled,
        )?;
        Ok(self.take_transition())
    }

    /// Stops a closing socket without waiting for payload-free send progress.
    ///
    /// Any already handed-off immutable wire frame retains its own bytes and
    /// original physical quota until the socket owner drops it.
    #[cfg(test)]
    pub(in crate::transport::websocket::session) fn disconnect(
        &mut self,
        controls: &mut WebSocketControlStaging,
    ) -> Result<WebSocketOperationTransition, WebSocketOperationSharedFailure> {
        self.disconnect_with(
            controls,
            chigiri_runtime::__private::SubscriptionTermination::TransportClosed,
        )
    }

    pub(in crate::transport::websocket::session) fn disconnect_with(
        &mut self,
        controls: &mut WebSocketControlStaging,
        decision: chigiri_runtime::__private::SubscriptionTermination,
    ) -> Result<WebSocketOperationTransition, WebSocketOperationSharedFailure> {
        self.cancel_inner(controls, decision)?;
        self.events.take();
        Ok(self.take_transition())
    }
}

fn invariant() -> WebSocketOperationSharedFailure {
    WebSocketOperationSharedFailure::Worker(
        crate::subscription::worker::SubscriptionWorkerRuntimeError::Invariant,
    )
}
