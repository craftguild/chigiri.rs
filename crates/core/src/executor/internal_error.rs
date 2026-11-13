//! Transfers executor-internal errors to facade diagnostic handling.

use crate::{FieldError, OperationIdentity, PathSegment, RequestTelemetry};

/// Unrecoverable framework failure observed during one execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum ExecutionFailureKind {
    /// A configured executor resource limit was exhausted.
    ResourceLimit,
    /// An executor invariant required to continue was violated.
    Invariant,
}

/// Internal contract for assigning IDs and notifying observers about internal
/// errors before and after completion.
#[doc(hidden)]
pub trait InternalErrorHandler: Send + Sync + 'static {
    /// Observes an internal error source before converting it into a client response.
    fn handle_internal_error(
        &self,
        error: &mut FieldError,
        identity: &OperationIdentity,
        telemetry: Option<&RequestTelemetry>,
        path: &[PathSegment],
        coordinate: Option<&str>,
        component: &'static str,
    );

    /// Reports a framework failure that makes the current execution unusable.
    ///
    /// The default keeps finite query and mutation integrations source
    /// compatible. Subscription runtimes can override it per event and stop
    /// the source without inspecting response messages or extension codes.
    fn handle_execution_failure(&self, _kind: ExecutionFailureKind) {}
}
