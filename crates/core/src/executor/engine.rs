//! Immutable state shared by one selection-set execution.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::executable::{CoercedVariables, CustomScalarRegistry, ValidatedDocument};
use crate::schema::SemanticSchema;
use crate::{
    ExecutionLimits, FieldError, GraphQlError, Location, Object, OperationIdentity, PathSegment,
    RequestTelemetry, Source, Value,
};

use super::{ExecutionFailureKind, FieldDispatch, InternalErrorHandler, Plugin};

pub(super) struct Engine<'a, C> {
    pub(super) schema: &'a SemanticSchema,
    pub(super) document: &'a ValidatedDocument,
    pub(super) variables: &'a CoercedVariables,
    pub(super) scalars: &'a CustomScalarRegistry,
    pub(super) input_budget: Option<&'a crate::executable::InputCoercionBudget>,
    pub(super) dispatch: &'a dyn FieldDispatch<C>,
    pub(super) plugins: &'a [Arc<dyn Plugin<C>>],
    pub(super) context: &'a C,
    pub(super) source: &'a Source,
    pub(super) limits: ExecutionLimits,
    pub(super) introspection_enabled: bool,
    pub(super) identity: &'a OperationIdentity,
    pub(super) telemetry: Option<&'a RequestTelemetry>,
    pub(super) internal_errors: Option<&'a dyn InternalErrorHandler>,
    pub(super) output_nodes: AtomicUsize,
    pub(super) output_bytes: AtomicUsize,
}

impl<C> Engine<'_, C> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new<'a>(
        schema: &'a SemanticSchema,
        document: &'a ValidatedDocument,
        variables: &'a CoercedVariables,
        scalars: &'a CustomScalarRegistry,
        input_budget: Option<&'a crate::executable::InputCoercionBudget>,
        dispatch: &'a dyn FieldDispatch<C>,
        plugins: &'a [Arc<dyn Plugin<C>>],
        context: &'a C,
        source: &'a Source,
        limits: ExecutionLimits,
        introspection_enabled: bool,
        identity: &'a OperationIdentity,
        telemetry: Option<&'a RequestTelemetry>,
        internal_errors: Option<&'a dyn InternalErrorHandler>,
    ) -> Engine<'a, C> {
        Engine {
            schema,
            document,
            variables,
            scalars,
            input_budget,
            dispatch,
            plugins,
            context,
            source,
            limits,
            introspection_enabled,
            identity,
            telemetry,
            internal_errors,
            output_nodes: AtomicUsize::new(0),
            output_bytes: AtomicUsize::new(0),
        }
    }

    /// Reserves one response node from the request-wide budget.
    pub(super) fn claim_output_node(&self) -> bool {
        self.claim_output_nodes(1)
    }

    /// Reserve all requested output nodes atomically without partial updates.
    pub(super) fn claim_output_nodes(&self, additional: usize) -> bool {
        claim_nodes(&self.output_nodes, self.limits.max_output_nodes, additional)
    }

    /// Convert an output-limit failure to a fixed internal error without exposing limit details.
    pub(super) fn output_limit_error(
        &self,
        path: Vec<PathSegment>,
        locations: Vec<Location>,
        bubble: bool,
        coordinate: Option<(&str, &str)>,
        component: &'static str,
    ) -> Completion {
        self.notify_execution_failure(ExecutionFailureKind::ResourceLimit);
        let error = self.graphql_error(
            FieldError::internal(OutputLimitExceeded { _private: () }),
            locations,
            path,
            coordinate,
            component,
        );
        Completion {
            value: Value::Null,
            errors: vec![error],
            bubble,
        }
    }

    /// Converts an internal error to a response after assigning an ID and
    /// notifying the observer, while retaining the source.
    pub(super) fn graphql_error(
        &self,
        mut error: FieldError,
        locations: Vec<Location>,
        path: Vec<PathSegment>,
        coordinate: Option<(&str, &str)>,
        component: &'static str,
    ) -> GraphQlError {
        if error.is_internal()
            && let Some(handler) = self.internal_errors
        {
            let coordinate = coordinate.map(|(parent, field)| format!("{parent}.{field}"));
            handler.handle_internal_error(
                &mut error,
                self.identity,
                self.telemetry,
                &path,
                coordinate.as_deref(),
                component,
            );
        }
        let error = error.to_graphql_error(locations, path);
        if !self.output_bytes_exhausted() {
            self.claim_output_bytes(error.message.len());
        }
        error
    }

    /// Notifies a request-scoped observer without changing the response.
    pub(super) fn notify_execution_failure(&self, kind: ExecutionFailureKind) {
        if let Some(handler) = self.internal_errors {
            handler.handle_execution_failure(kind);
        }
    }
}

pub(super) struct Completion {
    pub(super) value: Value,
    pub(super) errors: Vec<GraphQlError>,
    pub(super) bubble: bool,
}

impl Completion {
    /// Propagates owned-retention byte exhaustion to the operation entry point
    /// without creating a field error or an incorrect path.
    pub(super) fn exhausted() -> Self {
        Self {
            value: Value::Null,
            errors: Vec::new(),
            bubble: true,
        }
    }

    pub(super) fn value(value: Value) -> Self {
        Self {
            value,
            errors: Vec::new(),
            bubble: false,
        }
    }

    pub(super) fn null_with_error(error: GraphQlError) -> Self {
        Self {
            value: Value::Null,
            errors: vec![error],
            bubble: false,
        }
    }
}

pub(super) struct SelectionResult {
    pub(super) value: Object,
    pub(super) errors: Vec<GraphQlError>,
    pub(super) bubble: bool,
}

#[derive(Debug)]
/// Executor output resource limit; observers can classify it by type without
/// parsing this message.
pub struct OutputLimitExceeded {
    _private: (),
}

impl std::fmt::Display for OutputLimitExceeded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("response output limit exceeded")
    }
}

impl std::error::Error for OutputLimitExceeded {}

fn claim_nodes(counter: &AtomicUsize, maximum: usize, additional: usize) -> bool {
    if additional == 0 {
        return true;
    }
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current
                .checked_add(additional)
                .filter(|updated| *updated <= maximum)
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::claim_nodes;

    #[test]
    fn failed_batch_booking_preserves_counter_for_smaller_subsequent_bookings() {
        let counter = AtomicUsize::new(4);

        assert!(!claim_nodes(&counter, 5, 2));
        assert_eq!(counter.load(Ordering::Relaxed), 4);
        assert!(claim_nodes(&counter, 5, 1));
        assert_eq!(counter.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn batch_reservation_overflow_fails_closed() {
        let counter = AtomicUsize::new(usize::MAX);

        assert!(!claim_nodes(&counter, usize::MAX, 1));
        assert_eq!(counter.load(Ordering::Relaxed), usize::MAX);
        assert!(claim_nodes(&counter, usize::MAX, 0));
    }
}
