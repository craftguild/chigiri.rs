//! Revoked exact-generation terminal state with bounded physical cleanup.

use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri_http::DeadlineFactory;
use chigiri_runtime::GraphQlResponse;

use crate::subscription::bridge_resources::{
    ActiveBridgeOperation, BridgeCleanupCoordinator, BridgeCleanupOutcome, BridgeGeneration,
    BridgeResourceError, DetachedCleanup, PendingBridgeOperation, WebSocketOperationPhysicalLease,
};

use super::WebSocketSetupDeadline;

/// The single wire result selected when one operation becomes terminal.
pub(crate) enum WebSocketTerminalMessage {
    Error(GraphQlResponse),
    Complete,
    Cancelled,
}

/// Move-only terminal state after the logical ID and ordinary emission end.
pub(crate) struct WebSocketTerminalCarrier {
    generation: BridgeGeneration,
    message: Option<WebSocketTerminalMessage>,
    physical: Option<WebSocketOperationPhysicalLease>,
    cleanup: Option<Box<BridgeCleanupCoordinator<'static>>>,
    cleanup_outcome: Option<BridgeCleanupOutcome>,
}

impl WebSocketTerminalCarrier {
    /// Selects a task-free request rejection with the original setup timer.
    pub(crate) fn rejected_pending(
        operation: PendingBridgeOperation,
        response: GraphQlResponse,
        deadline: WebSocketSetupDeadline,
    ) -> Result<Self, BridgeResourceError> {
        let factory = deadline.factory();
        Self::pending(
            operation,
            WebSocketTerminalMessage::Error(response),
            DetachedCleanup::new(async move { drop(deadline) }),
            factory,
        )
    }

    pub(super) fn pending(
        operation: PendingBridgeOperation,
        message: WebSocketTerminalMessage,
        cleanup: DetachedCleanup,
        deadlines: &'static dyn DeadlineFactory,
    ) -> Result<Self, BridgeResourceError> {
        let generation = operation.generation();
        let terminal = operation.revoke()?;
        Ok(Self::new(
            generation,
            message,
            None,
            Some(terminal.coordinate(cleanup, deadlines)),
        ))
    }

    pub(super) fn active(
        operation: ActiveBridgeOperation,
        message: WebSocketTerminalMessage,
        cleanup: DetachedCleanup,
        deadlines: &'static dyn DeadlineFactory,
    ) -> Result<Self, BridgeResourceError> {
        let generation = operation.generation();
        let physical = operation.websocket_physical_lease()?;
        let terminal = operation.revoke()?;
        Ok(Self::new(
            generation,
            message,
            Some(physical),
            Some(terminal.coordinate(cleanup, deadlines)),
        ))
    }

    /// Constructs only a joined, task-free transition whose owners already ended.
    pub(super) fn completed(
        generation: BridgeGeneration,
        message: WebSocketTerminalMessage,
    ) -> Self {
        Self::new(generation, message, None, None)
    }

    #[cfg(test)]
    pub(crate) fn joined_for_test(
        operation: ActiveBridgeOperation,
        message: WebSocketTerminalMessage,
        deadlines: &'static dyn DeadlineFactory,
    ) -> Result<Self, BridgeResourceError> {
        Self::active(
            operation,
            message,
            DetachedCleanup::new(async {}),
            deadlines,
        )
    }

    fn new(
        generation: BridgeGeneration,
        message: WebSocketTerminalMessage,
        physical: Option<WebSocketOperationPhysicalLease>,
        cleanup: Option<BridgeCleanupCoordinator<'static>>,
    ) -> Self {
        Self {
            generation,
            message: Some(message),
            physical,
            cleanup: cleanup.map(Box::new),
            cleanup_outcome: None,
        }
    }

    pub(crate) fn generation(&self) -> BridgeGeneration {
        self.generation
    }

    pub(crate) fn take_message(&mut self) -> Option<WebSocketTerminalMessage> {
        self.message.take()
    }

    /// Replaces an unconsumed selected terminal with one safe operation error.
    pub(crate) fn replace_with_error(&mut self, response: GraphQlResponse) -> Result<(), ()> {
        let message = self.message.as_mut().ok_or(())?;
        if matches!(message, WebSocketTerminalMessage::Cancelled) {
            return Err(());
        }
        *message = WebSocketTerminalMessage::Error(response);
        Ok(())
    }

    /// Allows only an already-selected error or completion to drain prior events.
    pub(crate) fn permits_prior_event_drain(&self) -> bool {
        matches!(
            &self.message,
            Some(WebSocketTerminalMessage::Error(_) | WebSocketTerminalMessage::Complete)
        )
    }

    /// Moves only the original active-operation physical charge to wire staging.
    pub(crate) fn take_physical(&mut self) -> Option<WebSocketOperationPhysicalLease> {
        self.physical.take()
    }

    /// Shares only the original physical charge, with no logical authority.
    pub(crate) fn physical_lease(&self) -> Option<WebSocketOperationPhysicalLease> {
        self.physical.clone()
    }

    /// Drives the existing five-second cleanup and reports completion or quarantine.
    pub(crate) fn poll_cleanup(&mut self, context: &mut Context<'_>) -> Poll<BridgeCleanupOutcome> {
        if let Some(outcome) = self.cleanup_outcome {
            return Poll::Ready(outcome);
        }
        let Some(cleanup) = &mut self.cleanup else {
            self.cleanup_outcome = Some(BridgeCleanupOutcome::Completed);
            return Poll::Ready(BridgeCleanupOutcome::Completed);
        };
        let outcome = match Pin::new(cleanup.as_mut()).poll(context) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(outcome) => outcome,
        };
        self.cleanup.take();
        self.cleanup_outcome = Some(outcome);
        Poll::Ready(outcome)
    }
}
