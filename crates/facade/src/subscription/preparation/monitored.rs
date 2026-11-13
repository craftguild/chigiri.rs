//! Externally monitored context preparation and task-owned initial completion.

use super::*;
use crate::subscription::worker::SubscriptionWorkerRuntimeError;
use crate::transport::ContextTaskScope;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare_context_monitored<C, I>(
    schema: &ExecutableSchema<C>,
    factory: &Arc<dyn ErasedContextFactory<C>>,
    request: &SubscriberRequest,
    phase: RequestPhase,
    deadlines: &dyn DeadlineFactory,
    phase_deadline: &mut crate::subscription::worker::MonitoredDeadline,
    inherited: &mut I,
    telemetry: Option<&RequestTelemetry>,
) -> Result<ContextPreparation<C>, SubscriptionWorkerRuntimeError>
where
    C: Send + Sync + 'static,
    I: std::future::Future<Output = ()> + Send + Unpin,
{
    prepare_inner(
        schema,
        factory,
        request,
        phase,
        deadlines,
        phase_deadline,
        inherited,
        None,
        telemetry,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare_initial_context_monitored<C, I>(
    schema: &ExecutableSchema<C>,
    factory: &Arc<dyn ErasedContextFactory<C>>,
    request: &SubscriberRequest,
    phase: RequestPhase,
    deadlines: &dyn DeadlineFactory,
    phase_deadline: &mut crate::subscription::worker::MonitoredDeadline,
    inherited: &mut I,
    owner: &mut crate::subscription::worker::InitialTaskOwner<C>,
    telemetry: Option<&RequestTelemetry>,
) -> Result<ContextPreparation<C>, SubscriptionWorkerRuntimeError>
where
    C: Send + Sync + 'static,
    I: std::future::Future<Output = ()> + Send + Unpin,
{
    owner.arm(crate::subscription::driver::SubscriptionCompletion::empty());
    prepare_inner(
        schema,
        factory,
        request,
        phase,
        deadlines,
        phase_deadline,
        inherited,
        Some(owner),
        telemetry,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn prepare_inner<C, I>(
    schema: &ExecutableSchema<C>,
    factory: &Arc<dyn ErasedContextFactory<C>>,
    request: &SubscriberRequest,
    phase: RequestPhase,
    deadlines: &dyn DeadlineFactory,
    phase_deadline: &mut crate::subscription::worker::MonitoredDeadline,
    inherited: &mut I,
    owner: Option<&mut crate::subscription::worker::InitialTaskOwner<C>>,
    telemetry: Option<&RequestTelemetry>,
) -> Result<ContextPreparation<C>, SubscriptionWorkerRuntimeError>
where
    C: Send + Sync + 'static,
    I: std::future::Future<Output = ()> + Send + Unpin,
{
    let telemetry = telemetry
        .cloned()
        .unwrap_or_else(|| telemetry_for_phase(schema, request, phase));
    let observation = Phase::start(schema, LifecyclePhase::ContextConstruction, &telemetry);
    let headers = request.headers();
    let context = request.context(&headers, &telemetry, phase, deadlines);
    let scope = ContextTaskScope::subscription(phase_deadline.task_activity());
    let result = crate::subscription::worker::run_monitored(
        phase_deadline,
        inherited,
        observation.instrument(async {
            let mut prepared = factory.create_scoped(context, scope.clone()).await?;
            if let Some(owner) = owner {
                let token = prepared.take_completion();
                let completion = if request.transport() == SubscriptionTransport::WebSocket {
                    drop(token);
                    initial::InitialCompletion::empty()
                } else {
                    initial::InitialCompletion::new(token, &telemetry)
                };
                owner.arm(crate::subscription::driver::SubscriptionCompletion::armed(
                    completion,
                ));
            }
            Ok(prepared)
        }),
    )
    .await;
    if let Some(scope) = scope.filter(ContextTaskScope::failed) {
        if let Ok(prepared) = result {
            scope.dispose_preparation(prepared);
        }
        observation.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
        return Err(SubscriptionWorkerRuntimeError::Invariant);
    }
    Ok(match result {
        Ok(Ok(prepared)) => {
            let (execution_context, response_state, completion) = prepared.into_parts();
            observation.finish(LifecycleResult::Succeeded, None);
            ContextPreparation::Ready(FreshContext {
                execution_context,
                response_state,
                completion,
                telemetry,
            })
        }
        Ok(Err(PrepareError::Reject(rejection))) => {
            observation.finish(LifecycleResult::Rejected, None);
            ContextPreparation::Rejected {
                rejection,
                telemetry,
            }
        }
        Ok(Err(PrepareError::Internal(error))) => {
            observation.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
            ContextPreparation::Internal { error, telemetry }
        }
        Err(TimeoutKind::Phase | TimeoutKind::Overall) => {
            observation.finish(LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"));
            ContextPreparation::TimedOut { telemetry }
        }
    })
}
