//! Typed per-event failure state kept separate from client response rewriting.

use std::sync::atomic::{AtomicU8, Ordering};

use chigiri_core::executor::{ExecutionFailureKind, InternalErrorHandler};
use chigiri_core::{FieldError, OperationIdentity, PathSegment, RequestTelemetry};

use crate::diagnostics::Diagnostics;
use crate::subscription::SubscriptionTermination;

pub(super) struct EventDiagnostics {
    diagnostics: Diagnostics,
    failure: AtomicU8,
}

impl EventDiagnostics {
    pub(super) fn new(diagnostics: Diagnostics) -> Self {
        Self {
            diagnostics,
            failure: AtomicU8::new(0),
        }
    }

    pub(super) fn termination(&self) -> Option<SubscriptionTermination> {
        match self.failure.load(Ordering::Acquire) {
            1 => Some(SubscriptionTermination::ExecutionFailed),
            2 => Some(SubscriptionTermination::Internal),
            _ => None,
        }
    }

    fn record(&self, value: u8) {
        // Preserve the first executor-selected fatal class, without adding a
        // priority between independent ready policy/deadline conditions.
        let _ = self
            .failure
            .compare_exchange(0, value, Ordering::AcqRel, Ordering::Acquire);
    }
}

impl InternalErrorHandler for EventDiagnostics {
    fn handle_execution_failure(&self, kind: ExecutionFailureKind) {
        self.record(match kind {
            ExecutionFailureKind::ResourceLimit => 1,
            ExecutionFailureKind::Invariant => 2,
        });
    }

    fn handle_internal_error(
        &self,
        error: &mut FieldError,
        identity: &OperationIdentity,
        telemetry: Option<&RequestTelemetry>,
        path: &[PathSegment],
        coordinate: Option<&str>,
        component: &'static str,
    ) {
        if crate::private::is_generated_invariant(error) {
            self.record(2);
        }
        self.diagnostics
            .handle_internal_error(error, identity, telemetry, path, coordinate, component);
    }
}
