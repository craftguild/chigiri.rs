//! Resource-free fresh-context execution for one WebSocket Query or Mutation.

use chigiri_http::{RequestPhase, WebSocketOneShot};
use chigiri_runtime::__private::PreparedWebSocketOperation;
use chigiri_runtime::{GraphQlError, GraphQlResponse, LifecyclePhase, RequestTelemetry};

use super::OneShotTaskResult;
use crate::subscription::driver::input::ValidatedWebSocketOperation;
use crate::subscription::preparation::{ContextPreparation, prepare_context_monitored};
use crate::subscription::worker::{
    DeadlineArmer, DeadlineRole, SubscriptionWorkerRuntimeError, run_monitored,
};

pub(super) async fn run<C: Send + Sync + 'static>(
    validated: ValidatedWebSocketOperation<C>,
    armer: DeadlineArmer,
) -> OneShotTaskResult {
    let instrumentation = validated
        .request
        .request()
        .request_telemetry()
        .and_then(RequestTelemetry::instrumentation_scope)
        .cloned();
    let result = run_inner(validated, armer).await;
    if let Some(scope) = instrumentation {
        let outcome = match &result {
            Ok(response) if response.data.is_none() && !response.errors.is_empty() => {
                chigiri_runtime::LifecycleResult::Rejected
            }
            Ok(_) => chigiri_runtime::LifecycleResult::Succeeded,
            Err(_) => chigiri_runtime::LifecycleResult::Failed,
        };
        scope.finish(outcome);
    }
    result
}

async fn run_inner<C: Send + Sync + 'static>(
    validated: ValidatedWebSocketOperation<C>,
    armer: DeadlineArmer,
) -> OneShotTaskResult {
    let ValidatedWebSocketOperation {
        state,
        prepared,
        request,
    } = validated;
    let operation = match prepared {
        PreparedWebSocketOperation::OneShot(operation) => operation,
        PreparedWebSocketOperation::Subscription(_) => {
            return Err(SubscriptionWorkerRuntimeError::Invariant);
        }
    };
    let phase = match operation.kind() {
        chigiri_runtime::__private::OneShotOperationKind::Query => WebSocketOneShot::Query,
        chigiri_runtime::__private::OneShotOperationKind::Mutation => WebSocketOneShot::Mutation,
    };
    let mut setup = armer.setup()?;
    let mut context_deadline = armer.arm(DeadlineRole::Phase, state.settings.context_timeout)?;
    let fresh = prepare_context_monitored(
        &state.schema,
        &state.context_factory,
        &request,
        RequestPhase::WebSocketOperation { operation: phase },
        armer.factory(),
        &mut context_deadline,
        &mut setup,
        None,
    )
    .await?;
    if !matches!(fresh, ContextPreparation::TimedOut { .. }) {
        context_deadline.clear()?;
    }
    let fresh = match fresh {
        ContextPreparation::Ready(fresh) => fresh,
        ContextPreparation::Rejected { rejection, .. } => {
            setup.clear()?;
            return Ok(GraphQlResponse::request_error(rejection.error().clone()));
        }
        ContextPreparation::Internal { error, telemetry } => {
            setup.clear()?;
            return Ok(internal_failure(&state.schema, &error, &telemetry));
        }
        ContextPreparation::TimedOut { .. } => {
            return Ok(crate::timeout::request_timeout_response());
        }
    };
    let (context, context_tail) = fresh.into_websocket_execution();
    let mut execution_deadline =
        armer.arm(DeadlineRole::Phase, state.settings.execution_timeout)?;
    let response = match run_monitored(
        &mut execution_deadline,
        &mut setup,
        operation.execute(context, Some(armer.factory())),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => return Ok(crate::timeout::request_timeout_response()),
    };
    drop(context_tail);
    execution_deadline.clear()?;
    setup.clear()?;
    Ok(response)
}

fn internal_failure<C: Send + Sync + 'static>(
    schema: &chigiri_runtime::ExecutableSchema<C>,
    source: &(dyn std::error::Error + Send + Sync + 'static),
    telemetry: &RequestTelemetry,
) -> GraphQlResponse {
    let id = schema.observe_internal_error(
        LifecyclePhase::ContextConstruction,
        "INTERNAL_SERVER_ERROR",
        source,
        Some(telemetry),
        "websocket-operation-context",
    );
    let mut error = GraphQlError::new("Internal server error").code("INTERNAL_SERVER_ERROR");
    if let Some(id) = id {
        error = error.extension("errorId", id.as_str());
    }
    GraphQlResponse::request_error(error)
}
