//! Fresh application state for setup or one event execution.

use std::sync::Arc;

use crate::executable::InputCoercionBudget;
use crate::{DeadlineFactory, LifecycleObserver, RequestTelemetry};

use super::super::engine::Engine;
use super::super::{FieldDispatch, InternalErrorHandler, Plugin};
use super::PreparedSubscriptionOperation;

/// Borrowed fresh state that cannot replace prepared operation metadata.
#[doc(hidden)]
pub struct SubscriptionExecutionInput<'a, C> {
    pub(super) dispatch: &'a dyn FieldDispatch<C>,
    pub(super) plugins: &'a [Arc<dyn Plugin<C>>],
    pub(super) context: &'a C,
    pub(super) internal_errors: Option<&'a dyn InternalErrorHandler>,
    pub(super) lifecycle: Option<&'a dyn LifecycleObserver>,
    pub(super) telemetry: Option<&'a RequestTelemetry>,
    pub(super) deadlines: Option<&'a dyn DeadlineFactory>,
    pub(super) input_budget: Option<&'a InputCoercionBudget>,
}

impl<'a, C> SubscriptionExecutionInput<'a, C> {
    /// Creates fresh execution state without accepting operation-bound inputs.
    ///
    /// A prepared operation cannot be supplied here, so an event caller cannot
    /// replace the schema, document, selected operation, variables, or source
    /// retained by [`super::PreparedSubscriptionRoot`]. The former constructor
    /// shape is intentionally rejected:
    ///
    /// ```compile_fail
    /// # use std::sync::Arc;
    /// # use chigiri_core::executor::{FieldDispatch, Plugin};
    /// # use chigiri_core::executor::{PreparedSubscriptionOperation, SubscriptionExecutionInput};
    /// # fn cannot_mix<'a, C>(
    /// #     prepared: &'a PreparedSubscriptionOperation,
    /// #     dispatch: &'a dyn FieldDispatch<C>,
    /// #     plugins: &'a [Arc<dyn Plugin<C>>],
    /// #     context: &'a C,
    /// # ) {
    /// let _ = SubscriptionExecutionInput::new(
    ///     prepared, dispatch, plugins, context, None, None, None, None, None,
    /// );
    /// # }
    /// ```
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dispatch: &'a dyn FieldDispatch<C>,
        plugins: &'a [Arc<dyn Plugin<C>>],
        context: &'a C,
        internal_errors: Option<&'a dyn InternalErrorHandler>,
        lifecycle: Option<&'a dyn LifecycleObserver>,
        telemetry: Option<&'a RequestTelemetry>,
        deadlines: Option<&'a dyn DeadlineFactory>,
        input_budget: Option<&'a InputCoercionBudget>,
    ) -> Self {
        Self {
            dispatch,
            plugins,
            context,
            internal_errors,
            lifecycle,
            telemetry,
            deadlines,
            input_budget,
        }
    }

    pub(super) fn engine<'b>(
        &'b self,
        operation: &'b PreparedSubscriptionOperation,
    ) -> Engine<'b, C> {
        Engine::new(
            &operation.schema,
            &operation.document,
            &operation.variables,
            &operation.scalars,
            self.input_budget,
            self.dispatch,
            self.plugins,
            self.context,
            &operation.source,
            operation.limits,
            operation.introspection_enabled,
            &operation.identity,
            self.telemetry,
            self.internal_errors,
        )
    }
}
