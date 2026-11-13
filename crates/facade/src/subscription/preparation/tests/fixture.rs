use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chigiri_http::{
    BoxFuture, ContextFactory, HookError, PrepareError, PreparedContext, RequestContext,
    RequestPhase, ResponseCompletion, ResponseCompletionOutcome, ResponseHeaders, ResponseHook,
    ResponseHookContext, ResponseParts,
};
use chigiri_runtime::GraphQlError;

#[derive(Default)]
pub(super) struct Counts {
    pub(super) factory_calls: AtomicUsize,
    pub(super) hook_calls: AtomicUsize,
    pub(super) context_drops: AtomicUsize,
    pub(super) state_drops: AtomicUsize,
    pub(super) completion_calls: AtomicUsize,
    pub(super) completion_drops: AtomicUsize,
    pub(super) future_drops: AtomicUsize,
    pub(super) phases: Mutex<Vec<RequestPhase>>,
    pub(super) request_ids: Mutex<Vec<Option<String>>>,
    pub(super) response_error_counts: Mutex<Vec<usize>>,
}

pub(super) struct ExecutionContext(pub(super) Arc<Counts>);

impl Drop for ExecutionContext {
    fn drop(&mut self) {
        self.0.context_drops.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) struct HookState(pub(super) Arc<Counts>);

impl Drop for HookState {
    fn drop(&mut self) {
        self.0.state_drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct Completion(Arc<Counts>);

impl ResponseCompletion for Completion {
    fn complete(
        &self,
        mut response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        self.0.completion_calls.fetch_add(1, Ordering::SeqCst);
        response.headers.push(chigiri_http::ResponseHeader {
            name: "x-outcome".to_owned(),
            value: format!("{outcome:?}"),
        });
        response
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        self.0.completion_drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy)]
pub(super) enum FactoryMode {
    Ready,
    Reject,
    Internal,
    Pending,
}

pub(super) struct Factory {
    pub(super) counts: Arc<Counts>,
    pub(super) mode: FactoryMode,
}

impl ContextFactory<(), HookState> for Factory {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<(), HookState>, PrepareError>> {
        self.counts.factory_calls.fetch_add(1, Ordering::SeqCst);
        assert!(request.raw_body().is_empty());
        assert!(request.transport().deadline_factory().is_some());
        assert!(
            request
                .transport()
                .headers()
                .iter()
                .any(|header| header.name == "authorization" && header.value == b"original-secret")
        );
        self.counts.phases.lock().unwrap().push(request.phase());
        self.counts.request_ids.lock().unwrap().push(
            request
                .transport()
                .request_telemetry()
                .and_then(|telemetry| telemetry.request_id())
                .map(|id| id.as_str().to_owned()),
        );
        match self.mode {
            FactoryMode::Ready => {
                let counts = Arc::clone(&self.counts);
                Box::pin(async move {
                    Ok(PreparedContext::new((), HookState(Arc::clone(&counts)))
                        .with_response_completion(Completion(counts)))
                })
            }
            FactoryMode::Reject => Box::pin(async {
                Err(PrepareError::Reject(
                    chigiri_http::Rejection::new(403, GraphQlError::new("denied")).unwrap(),
                ))
            }),
            FactoryMode::Internal => Box::pin(async {
                Err(PrepareError::Internal(HookError::new(
                    std::io::Error::other("private"),
                )))
            }),
            FactoryMode::Pending => {
                let guard = FutureGuard(Arc::clone(&self.counts));
                Box::pin(async move {
                    pending::<()>().await;
                    drop(guard);
                    unreachable!()
                })
            }
        }
    }
}

struct FutureGuard(Arc<Counts>);

impl Drop for FutureGuard {
    fn drop(&mut self) {
        self.0.future_drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy)]
pub(super) enum HookMode {
    Ready,
    Internal,
    Pending,
}

pub(super) struct Hook {
    pub(super) counts: Arc<Counts>,
    pub(super) mode: HookMode,
}

impl ResponseHook<HookState> for Hook {
    fn call<'a>(
        &'a self,
        response: ResponseHookContext<'a, HookState>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        self.counts.hook_calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(self.counts.context_drops.load(Ordering::SeqCst), 1);
        assert!(matches!(
            response.request_context().phase(),
            RequestPhase::SubscriptionSetup { .. }
        ));
        self.counts
            .response_error_counts
            .lock()
            .unwrap()
            .push(response.response().errors.len());
        match self.mode {
            HookMode::Ready => Box::pin(async {
                let mut headers = ResponseHeaders::new();
                headers.append("x-subscription", "ready").unwrap();
                Ok(headers)
            }),
            HookMode::Internal => {
                Box::pin(async { Err(HookError::new(std::io::Error::other("private hook"))) })
            }
            HookMode::Pending => {
                let guard = FutureGuard(Arc::clone(&self.counts));
                Box::pin(async move {
                    pending::<()>().await;
                    drop(guard);
                    unreachable!()
                })
            }
        }
    }
}

pub(super) fn fresh(counts: Arc<Counts>) -> super::super::FreshContext<ExecutionContext> {
    super::super::FreshContext {
        execution_context: ExecutionContext(Arc::clone(&counts)),
        response_state: Box::new(HookState(Arc::clone(&counts))),
        completion: Some(Arc::new(Completion(counts))),
        telemetry: chigiri_runtime::RequestTelemetry::default(),
    }
}

pub(super) fn parts() -> ResponseParts {
    ResponseParts {
        status: 200,
        headers: Vec::new(),
        body: Vec::new(),
    }
}
