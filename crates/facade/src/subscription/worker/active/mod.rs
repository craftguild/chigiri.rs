//! Active source worker monitored independently from protocol body polling.

mod activate;
mod initial;
mod initial_command;
mod initial_completion;
mod initial_owner;
mod output;
mod setup;
mod state;
mod task;

pub(crate) use task::run_events;

use std::task::{Context, Poll};

use chigiri_http::{
    DeadlineFactory, ResponseCompletionOutcome, ResponseParts, SubscriptionTransport,
};

pub(crate) use initial::{InitialResponseDeadline, InitialResponseGate};
use initial_command::InitialResponseSupervisor;
pub(crate) use initial_owner::SourceAndInitialCompletion;
use output::WorkerSetupMessage;
pub(crate) use output::{
    ActiveSetupFailure, ActiveSetupFailureKind, ActiveSetupOutcome, ActiveWorkerSignal,
    SubscriptionWorkerEvent, SubscriptionWorkerSetup,
};

use super::port::{PortReceiver, PortSender};
use super::{ConcreteWorker, MonitoredDeadlineTable, SubscriptionWorkerRuntimeError};
use crate::subscription::bridge_resources::{ActiveBridgeOperation, BridgeCleanupCoordinator};
use crate::subscription::driver::SubscriptionOutput;
use crate::subscription::outbox::SubscriptionEventReservation;
use crate::subscription::worker::InitialWorkerResources;
use crate::transport::streaming::InitialRequestObservation;

pub(crate) struct ActiveSubscriptionWorker {
    pub(crate) activity: crate::managed::EventActivity,
    observation: Option<chigiri_runtime::__private::SubscriptionObservation>,
    operation: Option<ActiveBridgeOperation>,
    worker: Option<ConcreteWorker<Result<(), SubscriptionWorkerRuntimeError>>>,
    deadlines: Option<MonitoredDeadlineTable>,
    deadline_factory: &'static dyn DeadlineFactory,
    demand_sender: PortSender<()>,
    event_receiver: PortReceiver<SubscriptionOutput>,
    initial_response: InitialResponseSupervisor,
    initial_resources: Option<InitialWorkerResources>,
    pending_reservation: Option<SubscriptionEventReservation>,
    cleanup: Option<BridgeCleanupCoordinator<'static>>,
    terminal: Option<TerminalReason>,
    ended: bool,
    retained_telemetry: Option<chigiri_runtime::RequestTelemetry>,
}

pub(crate) struct ActiveWorkerSetup<C: Send + Sync + 'static> {
    worker: Option<ActiveSubscriptionWorker>,
    setup_receiver: PortReceiver<WorkerSetupMessage>,
    initial_buffer: Option<crate::subscription::worker::InitialResponseBufferLease>,
    observation: Option<InitialRequestObservation<C>>,
    transport: SubscriptionTransport,
}

#[derive(Clone, Copy)]
enum TerminalReason {
    TimedOut,
    Failed,
    Cancelled(ResponseCompletionOutcome),
}

impl ActiveSubscriptionWorker {
    pub(crate) fn source_delivery(
        &self,
    ) -> Option<std::sync::Arc<chigiri_runtime::__private::HostSourceDelivery>> {
        self.observation
            .as_ref()
            .and_then(|observation| observation.source_delivery())
    }
    pub(crate) fn retained_telemetry(&self) -> Option<&chigiri_runtime::RequestTelemetry> {
        self.retained_telemetry.as_ref()
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

    pub(crate) fn finish_observation(
        &mut self,
        decision: chigiri_runtime::__private::SubscriptionTermination,
        total: u64,
    ) -> bool {
        if let Some(observation) = &mut self.observation {
            observation.finish(decision, total);
            return true;
        }
        false
    }

    pub(crate) fn logical_terminal_selected(&self) -> bool {
        self.cleanup.is_some() || self.ended
    }

    pub(crate) fn cancel(&mut self) {
        self.begin_cleanup(self.managed_stop_reason().unwrap_or(TerminalReason::Failed));
    }

    fn begin_initial_completion(
        &mut self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> Result<(), SubscriptionWorkerRuntimeError> {
        self.initial_response.complete(response, outcome)
    }

    fn poll_initial_completion(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<ResponseParts, SubscriptionWorkerRuntimeError>> {
        match self.initial_response.poll_result(context) {
            Poll::Ready(Ok(response)) => {
                if let Some(initial) = &mut self.initial_resources {
                    initial.release_fallbacks();
                }
                Poll::Ready(Ok(response))
            }
            other => other,
        }
    }

    fn commit_initial_response(&mut self) -> Result<(), SubscriptionWorkerRuntimeError> {
        self.deadlines
            .as_ref()
            .ok_or(SubscriptionWorkerRuntimeError::Invariant)?
            .commit_initial_response()
    }

    fn cleanup_in_progress(&self) -> bool {
        self.cleanup.is_some()
    }

    pub(crate) fn request_event(
        &mut self,
        reservation: SubscriptionEventReservation,
    ) -> Result<(), SubscriptionEventReservation> {
        if let Some(wake) = self.request_event_deferred(reservation)? {
            wake.wake();
        }
        Ok(())
    }

    pub(crate) fn request_event_deferred(
        &mut self,
        reservation: SubscriptionEventReservation,
    ) -> Result<Option<std::task::Waker>, SubscriptionEventReservation> {
        if self.worker.is_none() || self.cleanup.is_some() || self.pending_reservation.is_some() {
            return Err(reservation);
        }
        self.pending_reservation = Some(reservation);
        match self.demand_sender.try_send_deferred(()) {
            Ok(wake) => Ok(wake),
            Err(()) => Err(self
                .pending_reservation
                .take()
                .expect("pending event reservation")),
        }
    }

    pub(crate) fn poll_signal(&mut self, context: &mut Context<'_>) -> Poll<ActiveWorkerSignal> {
        if let Poll::Ready(reason) = self.poll_terminal(context) {
            return Poll::Ready(reason);
        }
        if self.cleanup.is_some() {
            return Poll::Pending;
        }
        match self.event_receiver.poll_take(context) {
            Poll::Ready(Ok(Some(output))) => {
                let Some(reservation) = self.pending_reservation.take() else {
                    self.begin_cleanup(TerminalReason::Failed);
                    return self.poll_signal(context);
                };
                return Poll::Ready(ActiveWorkerSignal::Event(SubscriptionWorkerEvent::new(
                    reservation,
                    output,
                )));
            }
            Poll::Ready(Err(_)) => {
                self.begin_cleanup(TerminalReason::Failed);
                return self.poll_signal(context);
            }
            Poll::Ready(Ok(None)) | Poll::Pending => {}
        }
        match self.poll_join_output(context) {
            Poll::Ready(Ok(Some(event))) => Poll::Ready(ActiveWorkerSignal::Event(event)),
            Poll::Ready(Ok(None)) => Poll::Ready(ActiveWorkerSignal::Finished),
            Poll::Ready(Err(())) => Poll::Ready(ActiveWorkerSignal::Failed),
            Poll::Pending => Poll::Pending,
        }
    }
}
