//! Fresh application context preparation for subscription setup and events.

pub(crate) mod initial;
#[cfg(feature = "subscription-worker")]
mod monitored;
#[cfg(feature = "subscription-worker")]
pub(crate) use monitored::{prepare_context_monitored, prepare_initial_context_monitored};

use std::sync::Arc;
use std::time::Duration;

use chigiri_http::{
    DeadlineFactory, HookError, PrepareError, Rejection, RequestPhase, ResponseCompletion,
    SubscriptionTransport,
};
use chigiri_runtime::{ExecutableSchema, LifecyclePhase, LifecycleResult, RequestTelemetry};

use crate::subscription::request::SubscriberRequest;
use crate::transport::{ErasedContextFactory, Phase, TimeoutKind, run};

/// One phase-local application context and its otherwise opaque hook ownership.
pub(crate) struct FreshContext<C> {
    execution_context: C,
    response_state: Box<dyn std::any::Any + Send + Sync>,
    completion: Option<Arc<dyn ResponseCompletion>>,
    telemetry: RequestTelemetry,
}

impl<C> FreshContext<C> {
    pub(crate) const fn execution_context(&self) -> &C {
        &self.execution_context
    }

    pub(crate) const fn telemetry(&self) -> &RequestTelemetry {
        &self.telemetry
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn into_websocket_execution(self) -> (C, WebSocketContextTail) {
        let Self {
            execution_context,
            response_state,
            completion,
            telemetry,
        } = self;
        (
            execution_context,
            WebSocketContextTail {
                _response_state: response_state,
                _completion: completion,
                _telemetry: telemetry,
            },
        )
    }

    fn into_initial_parts(self) -> InitialParts {
        let Self {
            execution_context,
            response_state,
            completion,
            telemetry,
        } = self;
        drop(execution_context);
        InitialParts {
            response_state,
            completion,
            telemetry,
        }
    }

    pub(crate) fn into_failure_completion(
        self,
        transport: SubscriptionTransport,
    ) -> initial::InitialCompletion {
        let InitialParts {
            response_state,
            completion,
            telemetry,
        } = self.into_initial_parts();
        drop(response_state);
        if transport == SubscriptionTransport::WebSocket {
            drop(completion);
            initial::InitialCompletion::empty()
        } else {
            initial::InitialCompletion::new(completion, &telemetry)
        }
    }
}

#[cfg(feature = "subscription-worker")]
pub(crate) struct WebSocketContextTail {
    _response_state: Box<dyn std::any::Any + Send + Sync>,
    _completion: Option<Arc<dyn ResponseCompletion>>,
    _telemetry: RequestTelemetry,
}

pub(crate) enum ContextPreparation<C> {
    Ready(FreshContext<C>),
    Rejected {
        rejection: Rejection,
        telemetry: RequestTelemetry,
    },
    Internal {
        error: HookError,
        telemetry: RequestTelemetry,
    },
    TimedOut {
        telemetry: RequestTelemetry,
    },
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare_context<C>(
    schema: &ExecutableSchema<C>,
    factory: &Arc<dyn ErasedContextFactory<C>>,
    request: &SubscriberRequest,
    phase: RequestPhase,
    deadlines: &dyn DeadlineFactory,
    timeout: Duration,
    overall: std::pin::Pin<&mut (dyn std::future::Future<Output = ()> + Send)>,
    telemetry: Option<&RequestTelemetry>,
) -> ContextPreparation<C>
where
    C: Send + Sync + 'static,
{
    let telemetry = telemetry
        .cloned()
        .unwrap_or_else(|| telemetry_for_phase(schema, request, phase));
    let observation = Phase::start(schema, LifecyclePhase::ContextConstruction, &telemetry);
    let headers = request.headers();
    let context = request.context(&headers, &telemetry, phase, deadlines);
    match observation
        .instrument(run(deadlines, timeout, overall, async {
            factory.create(context).await
        }))
        .await
    {
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
    }
}

fn telemetry_for_phase<C>(
    schema: &ExecutableSchema<C>,
    request: &SubscriberRequest,
    phase: RequestPhase,
) -> RequestTelemetry
where
    C: Send + Sync + 'static,
{
    if matches!(
        phase,
        RequestPhase::SubscriptionSetup { .. } | RequestPhase::WebSocketOperation { .. }
    ) && let Some(telemetry) = request.request().request_telemetry()
    {
        return telemetry.clone();
    }
    schema.request_telemetry(request.trace_id().cloned())
}

struct InitialParts {
    response_state: Box<dyn std::any::Any + Send + Sync>,
    completion: Option<Arc<dyn ResponseCompletion>>,
    telemetry: RequestTelemetry,
}

#[cfg(test)]
mod tests;
