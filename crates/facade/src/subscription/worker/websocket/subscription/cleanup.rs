//! Immediate logical termination with bounded physical worker cleanup.

use chigiri_runtime::__private::SubscriptionTermination;
use chigiri_runtime::GraphQlResponse;

use super::ActiveWebSocketSubscription;
use crate::subscription::bridge_resources::DetachedCleanup;
use crate::subscription::failure::SubscriptionFailureClass;
use crate::subscription::worker::websocket::{WebSocketTerminalCarrier, WebSocketTerminalMessage};
use crate::subscription::worker::{SubscriptionWorkerRuntimeError, WorkerRetainedState};

#[derive(Clone, Copy)]
pub(super) enum SubscriptionTerminalCause {
    Complete,
    TimedOut,
    Failed(SubscriptionWorkerRuntimeError),
    Cancelled,
}

impl ActiveWebSocketSubscription {
    pub(super) fn terminal_response(
        &mut self,
        response: GraphQlResponse,
    ) -> Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError> {
        self.terminal_response_decision(response, SubscriptionTermination::ExecutionFailed, None)
    }

    pub(super) fn terminal_response_decision(
        &mut self,
        response: GraphQlResponse,
        decision: SubscriptionTermination,
        outbox: Option<&mut crate::subscription::outbox::SubscriptionOutbox>,
    ) -> Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError> {
        self.begin_terminal(WebSocketTerminalMessage::Error(response), decision, outbox)
    }

    pub(super) fn terminal_cause(
        &mut self,
        cause: SubscriptionTerminalCause,
    ) -> Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError> {
        self.terminal_cause_with_outbox(cause, None)
    }

    pub(super) fn terminal_cause_with_outbox(
        &mut self,
        cause: SubscriptionTerminalCause,
        outbox: Option<&mut crate::subscription::outbox::SubscriptionOutbox>,
    ) -> Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError> {
        let decision = match cause {
            SubscriptionTerminalCause::Complete => SubscriptionTermination::SourceComplete,
            SubscriptionTerminalCause::TimedOut => SubscriptionTermination::ExecutionFailed,
            SubscriptionTerminalCause::Failed(_) | SubscriptionTerminalCause::Cancelled => {
                SubscriptionTermination::Internal
            }
        };
        let message = match cause {
            SubscriptionTerminalCause::Complete => WebSocketTerminalMessage::Complete,
            SubscriptionTerminalCause::Cancelled => WebSocketTerminalMessage::Cancelled,
            SubscriptionTerminalCause::TimedOut => {
                let error = SubscriptionWorkerRuntimeError::Shutdown;
                WebSocketTerminalMessage::Error(
                    self.failure
                        .take()
                        .ok_or(SubscriptionWorkerRuntimeError::Invariant)?
                        .response(SubscriptionFailureClass::TimedOut, &error),
                )
            }
            SubscriptionTerminalCause::Failed(error) => WebSocketTerminalMessage::Error(
                self.failure
                    .take()
                    .ok_or(SubscriptionWorkerRuntimeError::Invariant)?
                    .response(super::super::output::worker_failure_class(error), &error),
            ),
        };
        self.begin_terminal(message, decision, outbox)
    }

    pub(super) fn begin_terminal(
        &mut self,
        message: WebSocketTerminalMessage,
        decision: SubscriptionTermination,
        outbox: Option<&mut crate::subscription::outbox::SubscriptionOutbox>,
    ) -> Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError> {
        let total = outbox.map(|outbox| {
            outbox.attach_source_delivery(self.source_delivery());
            let total = outbox.freeze_terminal_counter();
            self.observe_drops(outbox);
            total
        });
        if let Some(observation) = &mut self.observation {
            match total {
                Some(total) => observation.finish(decision, total),
                None => observation.finish_current(decision),
            }
        }
        self.demand_sender.close();
        self.event_receiver.close();
        let operation = self
            .operation
            .take()
            .ok_or(SubscriptionWorkerRuntimeError::Invariant)?;
        let deadlines = self.deadlines.take();
        let reservation = self.reservation.take();
        let cleanup = if let Some(worker) = self.worker.take() {
            worker.abort();
            let retained = WorkerRetainedState::websocket_active(deadlines, reservation);
            DetachedCleanup::worker(worker.erase(), retained)
        } else {
            DetachedCleanup::new(async move { drop((deadlines, reservation)) })
        };
        WebSocketTerminalCarrier::active(operation, message, cleanup, self.deadline_factory)
            .map_err(|_| SubscriptionWorkerRuntimeError::Invariant)
    }
}

impl Drop for ActiveWebSocketSubscription {
    fn drop(&mut self) {
        if self.operation.is_some() {
            drop(self.terminal_cause(SubscriptionTerminalCause::Cancelled));
        }
    }
}
