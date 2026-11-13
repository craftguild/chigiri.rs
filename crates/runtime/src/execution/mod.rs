//! Direct request execution and client-safe request-error conversion.

pub(crate) mod classification;
mod document;
mod errors;
mod instrumentation;
mod prepared;
mod validation;

pub(crate) use errors::{
    diagnostics_response, lifecycle_result, operation_identity, response_code, selection_error,
};
pub use prepared::{
    OneShotOperationKind, PreparedOneShotOperation, PreparedWebSocketOperation,
    WebSocketOperationKind,
};
pub(crate) use prepared::{
    PreparationPolicy, PreparedRequestPlan, retained_request_metadata_bytes,
};

use chigiri_core::executor::{BoxFuture, RequestContext, RequestNext};
use chigiri_core::{DeadlineFactory, GraphQlRequest, GraphQlResponse};

use crate::ExecutableSchema;

impl<C> ExecutableSchema<C>
where
    C: Send + Sync + 'static,
{
    /// Runs parsing, validation, coercion, and execution under direct-API limits.
    pub fn execute(&self, request: GraphQlRequest, context: C) -> BoxFuture<'_, GraphQlResponse> {
        self.execute_with_operation_policy(request, context, false, true, None)
    }

    /// The facade gives the same timer to the direct execution and the provider timeout.
    #[doc(hidden)]
    pub fn execute_with_deadlines<'a>(
        &'a self,
        request: GraphQlRequest,
        context: C,
        deadlines: &'a dyn DeadlineFactory,
    ) -> BoxFuture<'a, GraphQlResponse> {
        self.execute_with_operation_policy(request, context, false, true, Some(deadlines))
    }

    /// Restricts safe transport to queries and runs the shared request pipeline.
    #[doc(hidden)]
    pub fn execute_query_only(
        &self,
        request: GraphQlRequest,
        context: C,
    ) -> BoxFuture<'_, GraphQlResponse> {
        self.execute_with_operation_policy(request, context, true, true, None)
    }

    /// Executes a normal operation without duplicating the outer HTTP request event.
    #[doc(hidden)]
    pub fn execute_http<'a>(
        &'a self,
        request: GraphQlRequest,
        context: C,
        deadlines: &'a dyn DeadlineFactory,
    ) -> BoxFuture<'a, GraphQlResponse> {
        self.execute_with_operation_policy(request, context, false, false, Some(deadlines))
    }

    /// Executes a query without duplicating the outer HTTP request event.
    #[doc(hidden)]
    pub fn execute_query_only_http<'a>(
        &'a self,
        request: GraphQlRequest,
        context: C,
        deadlines: &'a dyn DeadlineFactory,
    ) -> BoxFuture<'a, GraphQlResponse> {
        self.execute_with_operation_policy(request, context, true, false, Some(deadlines))
    }

    async fn execute_with_operation_policy_inner(
        &self,
        mut request: GraphQlRequest,
        context: C,
        query_only: bool,
        direct: bool,
        deadlines: Option<&dyn DeadlineFactory>,
    ) -> GraphQlResponse {
        if request.request_telemetry().is_none() {
            request.telemetry = Some(self.inner.diagnostics.request_telemetry(None));
        }
        let request_phase = direct.then(|| {
            self.inner
                .diagnostics
                .start_request("direct", request.request_telemetry())
        });
        let request_context = RequestContext::new(&context, &request);
        let plugin_phase = self.inner.diagnostics.lifecycle().map(|_| {
            self.inner.diagnostics.start_phase(
                chigiri_core::LifecyclePhase::RequestPlugin,
                "chigiri",
                request.request_telemetry(),
            )
        });
        let response = RequestNext::new(&self.inner.plugins, |_| {
            Box::pin(self.execute_request(&request, &context, query_only, deadlines))
        })
        .run(request_context)
        .await;
        if let Some(phase) = plugin_phase {
            phase.finish(lifecycle_result(&response), None, response_code(&response));
        }
        if let Some(phase) = request_phase {
            phase.finish(lifecycle_result(&response), None, response_code(&response));
        }
        response
    }

    async fn execute_request(
        &self,
        request: &GraphQlRequest,
        context: &C,
        query_only: bool,
        deadlines: Option<&dyn DeadlineFactory>,
    ) -> GraphQlResponse {
        let policy = if query_only {
            prepared::PreparationPolicy::QueryOnly
        } else {
            prepared::PreparationPolicy::Finite
        };
        let prepared = match self.prepare_request_plan(request, policy, "chigiri", Ok) {
            Ok(prepared) => prepared,
            Err(response) => return *response,
        };
        self.run_prepared_plan(&prepared, context, request.request_telemetry(), deadlines)
            .await
    }
}
