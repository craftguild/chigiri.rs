//! Phase B source setup and the one sequential WebSocket subscription task.

mod activate;
mod cleanup;
mod emission;
mod setup;
mod task;

use std::pin::Pin;
use std::task::{Context, Poll};

use crate::subscription::bridge_resources::ActiveBridgeOperation;
use crate::subscription::driver::SubscriptionOutput;
use crate::subscription::failure::SubscriptionFailureReporter;
use crate::subscription::failure::outbox_failure_class;
use crate::subscription::outbox::{SubscriptionEventReservation, SubscriptionOutboxSignal};
use crate::subscription::worker::port::{PortReceiver, PortSender};
use crate::subscription::worker::{
    ConcreteWorker, MonitoredDeadlineTable, SubscriptionWorkerRuntimeError,
};
use cleanup::SubscriptionTerminalCause;

use super::WebSocketTerminalCarrier;
use super::output::WebSocketSubscriptionEvent;

pub(super) enum WebSocketSetupMessage {
    Observation(chigiri_runtime::__private::SubscriptionObservation),
    Ready {
        overflow: chigiri_runtime::SubscriptionOverflow,
        receipt: Option<chigiri_runtime::__private::SubscriptionObservationReceipt>,
        telemetry: Option<chigiri_runtime::RequestTelemetry>,
    },
    Response(chigiri_runtime::GraphQlResponse),
}

pub(crate) struct ActiveWebSocketSubscription {
    pub(crate) activity: crate::managed::EventActivity,
    observation: Option<chigiri_runtime::__private::SubscriptionObservation>,
    operation: Option<ActiveBridgeOperation>,
    worker: Option<ConcreteWorker<Result<(), SubscriptionWorkerRuntimeError>>>,
    deadlines: Option<MonitoredDeadlineTable>,
    deadline_factory: &'static dyn chigiri_http::DeadlineFactory,
    demand_sender: PortSender<()>,
    event_receiver: PortReceiver<SubscriptionOutput>,
    reservation: Option<SubscriptionEventReservation>,
    terminal_event_seen: bool,
    failure: Option<SubscriptionFailureReporter>,
}

pub(crate) struct WebSocketSubscriptionSetup {
    subscription: Option<ActiveWebSocketSubscription>,
    setup_receiver: PortReceiver<WebSocketSetupMessage>,
}

pub(crate) struct WebSocketSubscriptionTermination {
    terminal: Option<Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError>>,
}

impl WebSocketSubscriptionSetup {
    /// Revokes the logical ID immediately and returns its bounded cleanup owner.
    pub(crate) fn cancel(
        mut self,
        decision: chigiri_runtime::__private::SubscriptionTermination,
    ) -> WebSocketSubscriptionTermination {
        self.setup_receiver.close();
        let terminal = self
            .subscription
            .as_mut()
            .ok_or(SubscriptionWorkerRuntimeError::Invariant)
            .and_then(|subscription| {
                subscription.begin_terminal(
                    super::WebSocketTerminalMessage::Cancelled,
                    decision,
                    None,
                )
            });
        self.subscription.take();
        WebSocketSubscriptionTermination {
            terminal: Some(terminal),
        }
    }
}

impl ActiveWebSocketSubscription {
    pub(crate) fn source_delivery(
        &self,
    ) -> Option<std::sync::Arc<chigiri_runtime::__private::HostSourceDelivery>> {
        self.observation
            .as_ref()
            .and_then(|observation| observation.source_delivery())
    }
    pub(crate) fn observe_drops(
        &mut self,
        outbox: &mut crate::subscription::outbox::SubscriptionOutbox,
    ) {
        outbox.attach_source_delivery(self.source_delivery());
        for total in outbox.take_drop_observations() {
            if let Some(observation) = &mut self.observation {
                observation.drop_oldest(total);
            }
        }
    }

    pub(crate) fn request_event(
        &mut self,
        reservation: SubscriptionEventReservation,
    ) -> Result<(), SubscriptionEventReservation> {
        if self.reservation.is_some() || self.terminal_event_seen || self.operation.is_none() {
            return Err(reservation);
        }
        if self.demand_sender.try_send(()).is_err() {
            return Err(reservation);
        }
        self.reservation = Some(reservation);
        Ok(())
    }

    pub(crate) fn poll_event(
        &mut self,
        context: &mut Context<'_>,
        outbox: &mut crate::subscription::outbox::SubscriptionOutbox,
    ) -> Poll<Result<WebSocketSubscriptionEvent, SubscriptionWorkerRuntimeError>> {
        self.observe_drops(outbox);
        match self.poll_deadline(context, Some(outbox)) {
            Poll::Ready(terminal) => {
                return Poll::Ready(terminal.map(|terminal| {
                    self.terminal_event_seen = true;
                    WebSocketSubscriptionEvent::Terminal { terminal }
                }));
            }
            Poll::Pending => {}
        }
        match self.event_receiver.poll_take(context) {
            Poll::Ready(Ok(Some(SubscriptionOutput::Event(response)))) => {
                let reservation = self
                    .reservation
                    .take()
                    .ok_or(SubscriptionWorkerRuntimeError::Invariant)?;
                Poll::Ready(Ok(WebSocketSubscriptionEvent::Event {
                    reservation,
                    response,
                }))
            }
            Poll::Ready(Ok(Some(SubscriptionOutput::Terminal(response)))) => {
                self.terminal_event_seen = true;
                let (response, decision) = response.into_parts();
                let terminal = self.terminal_response_decision(response, decision, Some(outbox))?;
                Poll::Ready(Ok(WebSocketSubscriptionEvent::Terminal { terminal }))
            }
            Poll::Ready(Ok(Some(SubscriptionOutput::Complete))) => {
                self.terminal_event_seen = true;
                let terminal = self.terminal_cause_with_outbox(
                    SubscriptionTerminalCause::Complete,
                    Some(outbox),
                )?;
                Poll::Ready(Ok(WebSocketSubscriptionEvent::Terminal { terminal }))
            }
            Poll::Ready(Ok(Some(SubscriptionOutput::Cancelled))) => {
                self.terminal_event_seen = true;
                let terminal = self.terminal_cause_with_outbox(
                    SubscriptionTerminalCause::Cancelled,
                    Some(outbox),
                )?;
                Poll::Ready(Ok(WebSocketSubscriptionEvent::Terminal { terminal }))
            }
            Poll::Ready(Ok(None)) => {
                self.failed_event(SubscriptionWorkerRuntimeError::Shutdown, outbox)
            }
            Poll::Ready(Err(error)) => self.failed_event(error, outbox),
            Poll::Pending => Poll::Pending,
        }
    }

    pub(crate) fn cancel(
        mut self,
        decision: chigiri_runtime::__private::SubscriptionTermination,
        outbox: Option<&mut crate::subscription::outbox::SubscriptionOutbox>,
    ) -> WebSocketSubscriptionTermination {
        let terminal =
            self.begin_terminal(super::WebSocketTerminalMessage::Cancelled, decision, outbox);
        WebSocketSubscriptionTermination {
            terminal: Some(terminal),
        }
    }

    pub(crate) fn delivery_failure(
        &mut self,
        signal: SubscriptionOutboxSignal,
        outbox: Option<&mut crate::subscription::outbox::SubscriptionOutbox>,
    ) -> Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError> {
        let response = self
            .failure
            .as_ref()
            .ok_or(SubscriptionWorkerRuntimeError::Invariant)?
            .response(outbox_failure_class(signal), &signal);
        self.terminal_response_decision(
            response,
            crate::subscription::failure::outbox_termination(signal),
            outbox,
        )
    }

    fn poll_deadline(
        &mut self,
        context: &mut Context<'_>,
        outbox: Option<&mut crate::subscription::outbox::SubscriptionOutbox>,
    ) -> Poll<Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError>> {
        if let Some(observation) = &mut self.observation {
            observation.poll(context);
        }
        let expired = self
            .deadlines
            .as_mut()
            .ok_or(SubscriptionWorkerRuntimeError::Invariant)?
            .poll_expired(context);
        match expired {
            Poll::Ready(Ok(_)) => Poll::Ready(
                self.terminal_cause_with_outbox(SubscriptionTerminalCause::TimedOut, outbox),
            ),
            Poll::Ready(Err(error)) => Poll::Ready(
                self.terminal_cause_with_outbox(SubscriptionTerminalCause::Failed(error), outbox),
            ),
            Poll::Pending => Poll::Pending,
        }
    }

    fn failed_event(
        &mut self,
        error: SubscriptionWorkerRuntimeError,
        outbox: &mut crate::subscription::outbox::SubscriptionOutbox,
    ) -> Poll<Result<WebSocketSubscriptionEvent, SubscriptionWorkerRuntimeError>> {
        self.terminal_event_seen = true;
        Poll::Ready(
            self.terminal_cause_with_outbox(SubscriptionTerminalCause::Failed(error), Some(outbox))
                .map(|terminal| WebSocketSubscriptionEvent::Terminal { terminal }),
        )
    }
}

impl Future for WebSocketSubscriptionTermination {
    type Output = Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError>;

    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(
            self.terminal
                .take()
                .unwrap_or(Err(SubscriptionWorkerRuntimeError::Invariant)),
        )
    }
}
