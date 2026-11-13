use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chigiri_http::{
    BoxFuture, ContextFactory, HookError, PrepareError, PreparedContext, RequestContext,
    ResponseCompletion, ResponseCompletionOutcome, ResponseHeaders, ResponseHook,
    ResponseHookContext, ResponseParts,
};
use chigiri_runtime::{GraphQlError, RequestId, RequestIdFactory, RequestIdFactoryError};

use super::Probe;

pub(super) struct EventLocal {
    pub(super) cache: Mutex<Option<i32>>,
    pub(super) state: AtomicUsize,
    pub(super) budget: AtomicUsize,
}

pub(super) struct AppContext {
    pub(super) token: String,
    pub(super) service: Arc<Probe>,
    pub(super) local: EventLocal,
}

impl Drop for AppContext {
    fn drop(&mut self) {
        self.service.context_drops.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) struct HookState;

pub(super) struct Factory(pub(super) Arc<Probe>);

impl ContextFactory<AppContext, HookState> for Factory {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, HookState>, PrepareError>> {
        let token = request
            .transport()
            .headers()
            .iter()
            .find(|header| header.name == "authorization")
            .and_then(|header| std::str::from_utf8(header.value).ok())
            .unwrap_or_default()
            .to_owned();
        let phase = request.phase();
        let request_id = request
            .transport()
            .request_telemetry()
            .and_then(|telemetry| telemetry.request_id())
            .map(|id| id.as_str().to_owned())
            .unwrap();
        {
            let mut records = self.0.records.lock().unwrap();
            records.credentials.push(token.clone());
            records.phases.push(phase);
            records.request_ids.push(request_id);
        }
        if !self.0.allowed.lock().unwrap().contains(&token) {
            return Box::pin(async {
                Err(PrepareError::Reject(
                    chigiri_http::Rejection::new(
                        403,
                        GraphQlError::new("credential revoked").code("UNAUTHENTICATED"),
                    )
                    .unwrap(),
                ))
            });
        }
        let service = Arc::clone(&self.0);
        let completion_service = Arc::clone(&service);
        Box::pin(async move {
            Ok(PreparedContext::new(
                AppContext {
                    token,
                    service,
                    local: EventLocal {
                        cache: Mutex::new(None),
                        state: AtomicUsize::new(0),
                        budget: AtomicUsize::new(1),
                    },
                },
                HookState,
            )
            .with_response_completion(Completion(completion_service)))
        })
    }
}

pub(super) struct Hook(pub(super) Arc<Probe>);

impl ResponseHook<HookState> for Hook {
    fn call<'a>(
        &'a self,
        _: ResponseHookContext<'a, HookState>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        self.0.hook_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(ResponseHeaders::new()) })
    }
}

struct Completion(Arc<Probe>);

impl ResponseCompletion for Completion {
    fn complete(&self, response: ResponseParts, _: ResponseCompletionOutcome) -> ResponseParts {
        self.0.completion_calls.fetch_add(1, Ordering::SeqCst);
        response
    }
}

pub(super) struct RequestIds(pub(super) Arc<Probe>);

impl RequestIdFactory for RequestIds {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        let next = self.0.request_ids.fetch_add(1, Ordering::SeqCst) + 1;
        RequestId::new(format!("refresh-{next}"))
            .map_err(|error| RequestIdFactoryError::new(std::io::Error::other(error)))
    }
}
