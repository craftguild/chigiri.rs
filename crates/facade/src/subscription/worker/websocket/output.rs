//! Closed move-only results between workers and the connection supervisor.

use chigiri_runtime::__private::WebSocketOperationKind;
use chigiri_runtime::GraphQlResponse;
use std::task::{Context, Poll};

use super::deadlines::MonitoredWebSocketDeadlines;
use super::{WebSocketTerminalCarrier, WebSocketTerminalMessage};
use crate::subscription::bridge_resources::{
    BridgeResourceError, DetachedCleanup, PendingBridgeOperation,
};
use crate::subscription::driver::input::ValidatedWebSocketOperation;
use crate::subscription::failure::{
    SubscriptionFailureClass, SubscriptionFailureReporter, outbox_failure_class,
};
use crate::subscription::outbox::{SubscriptionEventReservation, SubscriptionOutboxSignal};
use crate::subscription::worker::SubscriptionWorkerRuntimeError;

pub(crate) struct PreparedWebSocketWorkerOperation<C> {
    pub(super) validated: ValidatedWebSocketOperation<C>,
    pub(super) operation: PendingBridgeOperation,
    pub(super) deadlines: MonitoredWebSocketDeadlines,
    pub(super) failure: SubscriptionFailureReporter,
}

impl<C> PreparedWebSocketWorkerOperation<C> {
    pub(crate) const fn kind(&self) -> WebSocketOperationKind
    where
        C: Send + Sync + 'static,
    {
        self.validated.prepared.kind()
    }

    /// Polls the one original setup deadline while active admission is paused.
    pub(crate) fn poll_setup_expired(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), SubscriptionWorkerRuntimeError>> {
        self.deadlines.poll_expired(context)
    }

    /// Selects a pending error and revokes the logical ID immediately.
    pub(crate) fn reject(
        self,
        response: GraphQlResponse,
    ) -> Result<WebSocketTerminalCarrier, BridgeResourceError> {
        let factory = self.deadlines.factory();
        let deadlines = self.deadlines.into_table();
        let cleanup = DetachedCleanup::new(async move { drop(deadlines) });
        WebSocketTerminalCarrier::pending(
            self.operation,
            WebSocketTerminalMessage::Error(response),
            cleanup,
            factory,
        )
    }

    /// Cancels before active admission without producing a wire terminal.
    pub(crate) fn cancel(self) -> Result<WebSocketTerminalCarrier, BridgeResourceError> {
        let factory = self.deadlines.factory();
        let deadlines = self.deadlines.into_table();
        WebSocketTerminalCarrier::pending(
            self.operation,
            WebSocketTerminalMessage::Cancelled,
            DetachedCleanup::new(async move { drop(deadlines) }),
            factory,
        )
    }

    /// Converts the original setup deadline result through the shared mapper.
    pub(crate) fn setup_failure(
        self,
        result: Result<(), SubscriptionWorkerRuntimeError>,
    ) -> Result<WebSocketTerminalCarrier, BridgeResourceError> {
        let (class, error) = match result {
            Ok(()) => (
                SubscriptionFailureClass::TimedOut,
                SubscriptionWorkerRuntimeError::Shutdown,
            ),
            Err(error) => (worker_failure_class(error), error),
        };
        let response = self.failure.response(class, &error);
        self.reject(response)
    }

    /// Converts a bounded delivery failure before active execution begins.
    pub(crate) fn delivery_failure(
        self,
        signal: SubscriptionOutboxSignal,
    ) -> Result<WebSocketTerminalCarrier, BridgeResourceError> {
        let response = self.failure.response(outbox_failure_class(signal), &signal);
        self.reject(response)
    }
}

pub(crate) struct JoinedWebSocketOneShot {
    reservation: Option<SubscriptionEventReservation>,
    response: Option<GraphQlResponse>,
    terminal: Option<WebSocketTerminalCarrier>,
}

impl JoinedWebSocketOneShot {
    pub(super) const fn new(
        reservation: SubscriptionEventReservation,
        response: GraphQlResponse,
        terminal: WebSocketTerminalCarrier,
    ) -> Self {
        Self {
            reservation: Some(reservation),
            response: Some(response),
            terminal: Some(terminal),
        }
    }

    /// Moves the pre-terminal reservation and single response to bounded encoding.
    pub(crate) fn take_event(&mut self) -> Option<(SubscriptionEventReservation, GraphQlResponse)> {
        Some((self.reservation.take()?, self.response.take()?))
    }

    /// Returns proof that terminal selection already revoked the exact generation.
    #[cfg(test)]
    pub(crate) fn terminal(&self) -> Option<&WebSocketTerminalCarrier> {
        self.terminal.as_ref()
    }

    /// Moves the terminal only after the single pre-terminal event was consumed.
    pub(crate) fn take_terminal(
        mut self,
    ) -> Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError> {
        if self.reservation.is_some() || self.response.is_some() {
            return Err(SubscriptionWorkerRuntimeError::Invariant);
        }
        self.terminal
            .take()
            .ok_or(SubscriptionWorkerRuntimeError::Invariant)
    }
}

pub(crate) enum WebSocketOneShotOutcome {
    Ready(JoinedWebSocketOneShot),
    Terminal(WebSocketTerminalCarrier),
    SharedFailure(SubscriptionWorkerRuntimeError),
}

pub(crate) struct WebSocketValidationStartFailure {
    pub(super) operation: Option<PendingBridgeOperation>,
    pub(super) deadlines: Option<MonitoredWebSocketDeadlines>,
    pub(super) factory: &'static dyn chigiri_http::DeadlineFactory,
    pub(super) cause: WebSocketValidationStartFailureCause,
    pub(super) failure: Option<SubscriptionFailureReporter>,
}

pub(super) enum WebSocketValidationStartFailureCause {
    Worker(SubscriptionWorkerRuntimeError),
    Resource(BridgeResourceError),
}

impl WebSocketValidationStartFailure {
    #[cfg(test)]
    pub(crate) const fn retains_original_deadline(&self) -> bool {
        self.deadlines.is_some()
    }

    /// Revokes the ID and returns a safe task-free terminal plus bounded cleanup.
    pub(crate) fn into_terminal(mut self) -> Result<WebSocketTerminalCarrier, BridgeResourceError> {
        let operation = self
            .operation
            .take()
            .ok_or(BridgeResourceError::Invariant)?;
        let failure = self.failure.take().ok_or(BridgeResourceError::Invariant)?;
        let response = match self.cause {
            WebSocketValidationStartFailureCause::Worker(error) => {
                failure.response(worker_failure_class(error), &error)
            }
            WebSocketValidationStartFailureCause::Resource(error) => {
                failure.response(resource_failure_class(error), &error)
            }
        };
        let deadlines = self
            .deadlines
            .take()
            .map(MonitoredWebSocketDeadlines::into_table);
        let cleanup = DetachedCleanup::new(async move { drop(deadlines) });
        WebSocketTerminalCarrier::pending(
            operation,
            WebSocketTerminalMessage::Error(response),
            cleanup,
            self.factory,
        )
    }
}

pub(crate) enum WebSocketValidationOutcome<C> {
    Prepared(Box<PreparedWebSocketWorkerOperation<C>>),
    Terminal(WebSocketTerminalCarrier),
    SharedFailure(SubscriptionWorkerRuntimeError),
}

pub(super) const fn worker_failure_class(
    error: SubscriptionWorkerRuntimeError,
) -> SubscriptionFailureClass {
    match error {
        SubscriptionWorkerRuntimeError::Capacity => SubscriptionFailureClass::Limit,
        _ => SubscriptionFailureClass::Internal,
    }
}

pub(super) const fn resource_failure_class(error: BridgeResourceError) -> SubscriptionFailureClass {
    match error {
        BridgeResourceError::Capacity => SubscriptionFailureClass::Limit,
        _ => SubscriptionFailureClass::Internal,
    }
}

pub(crate) enum WebSocketSubscriptionEvent {
    Event {
        reservation: SubscriptionEventReservation,
        response: GraphQlResponse,
    },
    Terminal {
        terminal: WebSocketTerminalCarrier,
    },
}

impl WebSocketSubscriptionEvent {
    #[cfg(test)]
    pub(crate) fn into_event(
        self,
    ) -> Result<(SubscriptionEventReservation, GraphQlResponse), Box<Self>> {
        match self {
            Self::Event {
                reservation,
                response,
            } => Ok((reservation, response)),
            terminal => Err(Box::new(terminal)),
        }
    }

    #[cfg(test)]
    pub(crate) fn into_terminal(self) -> Result<WebSocketTerminalCarrier, Box<Self>> {
        match self {
            Self::Terminal { terminal } => Ok(terminal),
            event => Err(Box::new(event)),
        }
    }
}

pub(crate) enum WebSocketSubscriptionSetupOutcome {
    Ready {
        subscription: Box<super::ActiveWebSocketSubscription>,
        overflow: chigiri_runtime::SubscriptionOverflow,
    },
    Terminal(WebSocketTerminalCarrier),
    SharedFailure(SubscriptionWorkerRuntimeError),
}
