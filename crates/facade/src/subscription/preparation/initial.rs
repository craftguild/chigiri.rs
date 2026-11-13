//! Setup-only HTTP response-hook and completion ownership.

mod instrumentation;

use std::sync::Arc;
use std::time::Duration;

use chigiri_http::{
    DeadlineFactory, HookError, ResponseCompletion, ResponseCompletionOutcome, ResponseHeaders,
    ResponseParts, SubscriptionTransport,
};
use chigiri_runtime::{GraphQlResponse, RequestTelemetry};

use super::{FreshContext, InitialParts};
use crate::subscription::request::SubscriberRequest;
use crate::transport::{ErasedResponseHook, TimeoutKind, run};

pub(crate) struct InitialCompletion {
    completion: Option<Arc<dyn ResponseCompletion>>,
}

impl InitialCompletion {
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn into_token(self) -> Option<Arc<dyn ResponseCompletion>> {
        self.completion
    }

    pub(crate) const fn empty() -> Self {
        Self { completion: None }
    }

    pub(super) fn new(
        completion: Option<Arc<dyn ResponseCompletion>>,
        telemetry: &RequestTelemetry,
    ) -> Self {
        Self {
            completion: instrumentation::completion(completion, telemetry),
        }
    }

    pub(crate) fn finish(
        mut self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        match self.completion.take() {
            Some(completion) => completion.complete(response, outcome),
            None => response,
        }
    }
}

pub(crate) struct PreparedInitialResponse {
    headers: ResponseHeaders,
    completion: InitialCompletion,
    telemetry: RequestTelemetry,
}

impl PreparedInitialResponse {
    pub(crate) fn into_parts(self) -> (ResponseHeaders, InitialCompletion, RequestTelemetry) {
        (self.headers, self.completion, self.telemetry)
    }
}

pub(crate) enum InitialResponsePreparation {
    Ready(PreparedInitialResponse),
    HookFailed {
        error: HookError,
        completion: InitialCompletion,
        telemetry: RequestTelemetry,
    },
    TimedOut {
        completion: InitialCompletion,
        telemetry: RequestTelemetry,
    },
    WebSocket {
        telemetry: RequestTelemetry,
    },
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare_initial_response<C>(
    fresh: FreshContext<C>,
    request: &SubscriberRequest,
    response: &GraphQlResponse,
    hook: Option<&Arc<dyn ErasedResponseHook>>,
    deadlines: &dyn DeadlineFactory,
    timeout: Duration,
    overall: std::pin::Pin<&mut (dyn std::future::Future<Output = ()> + Send)>,
) -> InitialResponsePreparation {
    let InitialParts {
        response_state,
        completion,
        telemetry,
    } = fresh.into_initial_parts();
    if request.transport() == SubscriptionTransport::WebSocket {
        drop(response_state);
        drop(completion);
        return InitialResponsePreparation::WebSocket { telemetry };
    }
    let completion = InitialCompletion::new(completion, &telemetry);
    let Some(hook) = hook else {
        return InitialResponsePreparation::Ready(PreparedInitialResponse {
            headers: ResponseHeaders::new(),
            completion,
            telemetry,
        });
    };
    let headers = request.headers();
    let context = request.context(
        &headers,
        &telemetry,
        chigiri_http::RequestPhase::SubscriptionSetup {
            transport: request.transport(),
        },
        deadlines,
    );
    match run(
        deadlines,
        timeout,
        overall,
        hook.call(context, response, response_state.as_ref()),
    )
    .await
    {
        Ok(Ok(headers)) => InitialResponsePreparation::Ready(PreparedInitialResponse {
            headers,
            completion,
            telemetry,
        }),
        Ok(Err(error)) => InitialResponsePreparation::HookFailed {
            error,
            completion,
            telemetry,
        },
        Err(TimeoutKind::Phase | TimeoutKind::Overall) => InitialResponsePreparation::TimedOut {
            completion,
            telemetry,
        },
    }
}

#[cfg(feature = "subscription-worker")]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare_initial_response_monitored<C, I>(
    fresh: FreshContext<C>,
    request: &SubscriberRequest,
    response: &GraphQlResponse,
    hook: Option<&Arc<dyn ErasedResponseHook>>,
    deadlines: &dyn DeadlineFactory,
    phase_deadline: &mut crate::subscription::worker::MonitoredDeadline,
    inherited: &mut I,
) -> InitialResponsePreparation
where
    I: std::future::Future<Output = ()> + Send + Unpin,
{
    let InitialParts {
        response_state,
        completion,
        telemetry,
    } = fresh.into_initial_parts();
    if request.transport() == SubscriptionTransport::WebSocket {
        drop(response_state);
        drop(completion);
        return InitialResponsePreparation::WebSocket { telemetry };
    }
    let completion = InitialCompletion::new(completion, &telemetry);
    let Some(hook) = hook else {
        return InitialResponsePreparation::Ready(PreparedInitialResponse {
            headers: ResponseHeaders::new(),
            completion,
            telemetry,
        });
    };
    let headers = request.headers();
    let context = request.context(
        &headers,
        &telemetry,
        chigiri_http::RequestPhase::SubscriptionSetup {
            transport: request.transport(),
        },
        deadlines,
    );
    match crate::subscription::worker::run_monitored(
        phase_deadline,
        inherited,
        hook.call(context, response, response_state.as_ref()),
    )
    .await
    {
        Ok(Ok(headers)) => InitialResponsePreparation::Ready(PreparedInitialResponse {
            headers,
            completion,
            telemetry,
        }),
        Ok(Err(error)) => InitialResponsePreparation::HookFailed {
            error,
            completion,
            telemetry,
        },
        Err(TimeoutKind::Phase | TimeoutKind::Overall) => InitialResponsePreparation::TimedOut {
            completion,
            telemetry,
        },
    }
}
