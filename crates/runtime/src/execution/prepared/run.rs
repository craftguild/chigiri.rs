//! Execution of an immutable plan with fresh caller-owned application state.

use chigiri_core::executor::execute_prepared_operation;
use chigiri_core::{DeadlineFactory, GraphQlResponse};

use super::PreparedRequestPlan;
use crate::ExecutableSchema;

impl<C: Send + Sync + 'static> ExecutableSchema<C> {
    pub(crate) async fn run_prepared_plan(
        &self,
        prepared: &PreparedRequestPlan,
        context: &C,
        telemetry: Option<&chigiri_core::RequestTelemetry>,
        deadlines: Option<&dyn DeadlineFactory>,
    ) -> GraphQlResponse {
        match execute_prepared_operation(
            &prepared.operation,
            self.inner.resolvers.as_ref(),
            &self.inner.plugins,
            context,
            Some(&self.inner.diagnostics),
            self.inner.diagnostics.lifecycle(),
            telemetry,
            deadlines,
            prepared.input_budget.as_ref(),
        ) {
            Ok(execution) => execution.await,
            Err(error) => crate::execution::selection_error(error),
        }
    }
}
