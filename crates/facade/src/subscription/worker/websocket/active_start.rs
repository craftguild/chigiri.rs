//! Closed terminal conversion for failures before an active worker starts.

use crate::subscription::bridge_resources::{
    ActiveBridgeOperation, BridgeGeneration, BridgeResourceError, DetachedCleanup,
    PendingBridgeOperation,
};
use crate::subscription::failure::SubscriptionFailureReporter;
use crate::subscription::outbox::SubscriptionEventReservation;
use crate::subscription::worker::{MonitoredDeadlineTable, SubscriptionWorkerRuntimeError};
use chigiri_http::DeadlineFactory;

use super::deadlines::MonitoredWebSocketDeadlines;
use super::output::resource_failure_class;
use super::{WebSocketTerminalCarrier, WebSocketTerminalMessage};

pub(crate) struct WebSocketActiveStartFailure {
    #[cfg(test)]
    error: BridgeResourceError,
    terminal: Option<Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError>>,
}

impl WebSocketActiveStartFailure {
    pub(super) fn pending_completed(
        error: BridgeResourceError,
        operation: PendingBridgeOperation,
        deadlines: MonitoredWebSocketDeadlines,
        reservation: Option<SubscriptionEventReservation>,
        failure: SubscriptionFailureReporter,
    ) -> Self {
        let generation = operation.generation();
        let response = failure.response(resource_failure_class(error), &error);
        drop((operation, deadlines, reservation));
        Self {
            #[cfg(test)]
            error,
            terminal: Some(Ok(WebSocketTerminalCarrier::completed(
                generation,
                WebSocketTerminalMessage::Error(response),
            ))),
        }
    }

    pub(super) fn completed(
        error: BridgeResourceError,
        generation: BridgeGeneration,
        deadlines: MonitoredDeadlineTable,
        reservation: Option<SubscriptionEventReservation>,
        failure: SubscriptionFailureReporter,
    ) -> Self {
        let response = failure.response(resource_failure_class(error), &error);
        drop((deadlines, reservation));
        Self {
            #[cfg(test)]
            error,
            terminal: Some(Ok(WebSocketTerminalCarrier::completed(
                generation,
                WebSocketTerminalMessage::Error(response),
            ))),
        }
    }

    pub(super) fn active(
        error: BridgeResourceError,
        operation: ActiveBridgeOperation,
        deadlines: MonitoredDeadlineTable,
        reservation: Option<SubscriptionEventReservation>,
        failure: SubscriptionFailureReporter,
        factory: &'static dyn DeadlineFactory,
    ) -> Self {
        let response = failure.response(resource_failure_class(error), &error);
        let cleanup = DetachedCleanup::new(async move { drop((deadlines, reservation)) });
        Self {
            #[cfg(test)]
            error,
            terminal: Some(
                WebSocketTerminalCarrier::active(
                    operation,
                    WebSocketTerminalMessage::Error(response),
                    cleanup,
                    factory,
                )
                .map_err(|_| SubscriptionWorkerRuntimeError::Invariant),
            ),
        }
    }

    #[cfg(test)]
    pub(crate) const fn error(&self) -> BridgeResourceError {
        self.error
    }

    pub(crate) fn into_terminal(
        mut self,
    ) -> Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError> {
        self.terminal
            .take()
            .unwrap_or(Err(SubscriptionWorkerRuntimeError::Invariant))
    }
}
