//! Shared context, response hook, completion and request-identity fixtures.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};

use chigiri_http::{
    BoxFuture, ContextFactory, HookError, PrepareError, PreparedContext, RequestContext,
    RequestPhase, ResponseCompletion, ResponseCompletionOutcome, ResponseHeaders, ResponseHook,
    ResponseHookContext, ResponseParts,
};
use chigiri_runtime::{GraphQlError, RequestId, RequestIdFactory, RequestIdFactoryError};

use super::super::state::{AppContext, ContextKind, FactoryMode, HookMode, HookState, State};

pub(in crate::subscription::driver::tests) struct Factory(
    pub(in crate::subscription::driver::tests) Arc<State>,
);

impl ContextFactory<AppContext, HookState> for Factory {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, HookState>, PrepareError>> {
        self.0.counts.factory_calls.fetch_add(1, Ordering::SeqCst);
        assert!(request.raw_body().is_empty());
        assert!(request.transport().deadline_factory().is_some());
        assert!(
            request
                .transport()
                .headers()
                .iter()
                .any(|header| header.name == "authorization" && header.value == b"original-secret")
        );
        let phase = request.phase();
        self.0.phases.lock().unwrap().push(phase);
        self.0.phase_request_ids.lock().unwrap().push(
            request
                .transport()
                .request_telemetry()
                .and_then(|telemetry| telemetry.request_id())
                .map(|id| id.as_str().to_owned()),
        );
        let (kind, mode) = match phase {
            RequestPhase::WebSocketOperation { .. } => (ContextKind::Setup, self.0.setup_factory),
            RequestPhase::SubscriptionSetup { .. } => (ContextKind::Setup, self.0.setup_factory),
            RequestPhase::SubscriptionEvent { .. } => (ContextKind::Event, self.0.event_factory),
            _ => panic!("subscription driver used a non-subscription context phase"),
        };
        match mode {
            FactoryMode::Ready => {
                let state = Arc::clone(&self.0);
                let generation = state.counts.factory_calls.load(Ordering::SeqCst);
                Box::pin(async move {
                    Ok(PreparedContext::new(
                        AppContext {
                            generation,
                            kind,
                            state: Arc::clone(&state),
                        },
                        HookState {
                            kind,
                            state: Arc::clone(&state),
                        },
                    )
                    .with_response_completion(Completion(state)))
                })
            }
            FactoryMode::Reject => Box::pin(async {
                Err(PrepareError::Reject(
                    chigiri_http::Rejection::new(403, GraphQlError::new("context denied")).unwrap(),
                ))
            }),
            FactoryMode::Internal => Box::pin(async {
                Err(PrepareError::Internal(HookError::new(
                    std::io::Error::other("private context"),
                )))
            }),
            FactoryMode::Pending => Box::pin(PendingFactory(Arc::clone(&self.0))),
            #[cfg(feature = "subscription-worker")]
            FactoryMode::Panic => Box::pin(async { panic!("fixture context panic") }),
        }
    }
}

pub(in crate::subscription::driver::tests) struct Hook(
    pub(in crate::subscription::driver::tests) Arc<State>,
);

impl ResponseHook<HookState> for Hook {
    fn call<'a>(
        &'a self,
        response: ResponseHookContext<'a, HookState>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        self.0.counts.hook_calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(self.0.counts.setup_context_drops.load(Ordering::SeqCst), 1);
        self.0
            .hook_error_counts
            .lock()
            .unwrap()
            .push(response.response().errors.len());
        match self.0.hook_mode {
            HookMode::Ready => Box::pin(async {
                let mut headers = ResponseHeaders::new();
                headers.append("x-subscription", "ready").unwrap();
                Ok(headers)
            }),
            HookMode::Internal => {
                Box::pin(async { Err(HookError::new(std::io::Error::other("private hook"))) })
            }
            HookMode::Pending => Box::pin(PendingHook(Arc::clone(&self.0))),
        }
    }
}

struct Completion(Arc<State>);

impl ResponseCompletion for Completion {
    fn complete(
        &self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        self.0
            .counts
            .completion_calls
            .fetch_add(1, Ordering::SeqCst);
        self.0.completion_outcomes.lock().unwrap().push(outcome);
        self.0.completion_drop_counts.lock().unwrap().push([
            self.0.counts.source_drops.load(Ordering::SeqCst),
            self.0.counts.setup_future_drops.load(Ordering::SeqCst),
            self.0.counts.hook_future_drops.load(Ordering::SeqCst),
            self.0.counts.setup_context_drops.load(Ordering::SeqCst),
        ]);
        response
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        self.0
            .counts
            .completion_drops
            .fetch_add(1, Ordering::SeqCst);
    }
}

struct PendingFactory(Arc<State>);

impl Future for PendingFactory {
    type Output = Result<PreparedContext<AppContext, HookState>, PrepareError>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for PendingFactory {
    fn drop(&mut self) {
        self.0
            .counts
            .factory_future_drops
            .fetch_add(1, Ordering::SeqCst);
    }
}

struct PendingHook(Arc<State>);

impl Future for PendingHook {
    type Output = Result<ResponseHeaders, HookError>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for PendingHook {
    fn drop(&mut self) {
        self.0
            .counts
            .hook_future_drops
            .fetch_add(1, Ordering::SeqCst);
    }
}

pub(in crate::subscription::driver::tests) struct RequestIds(
    pub(in crate::subscription::driver::tests) Arc<State>,
);

impl RequestIdFactory for RequestIds {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        let call = self.0.counts.request_ids.fetch_add(1, Ordering::SeqCst) + 1;
        RequestId::new(format!("request-{call}"))
            .map_err(|error| RequestIdFactoryError::new(std::io::Error::other(error)))
    }
}
