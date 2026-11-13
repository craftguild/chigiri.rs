//! Pass encoded responses and failure paths to synchronized completion exactly once.

use super::*;
use chigiri_http::{ResponseCompletion, ResponseCompletionOutcome, ResponseParts};

type CompletionRecords = Vec<(ResponseCompletionOutcome, Vec<u8>)>;

#[derive(Clone, Default)]
struct Completion(Arc<Mutex<CompletionRecords>>);

impl ResponseCompletion for Completion {
    fn complete(
        &self,
        mut response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        self.0
            .lock()
            .unwrap()
            .push((outcome, response.body.clone()));
        response.headers.push(chigiri_http::ResponseHeader {
            name: "x-completed".into(),
            value: "yes".into(),
        });
        response
    }
}

struct CompletionFactory(Completion);

impl ContextFactory<TestContext, ()> for CompletionFactory {
    fn create<'a>(
        &'a self,
        _: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<TestContext, ()>, PrepareError>> {
        Box::pin(async move {
            Ok(PreparedContext::execution_only(TestContext {
                log: PhaseLog::default(),
            })
            .with_response_completion(self.0.clone()))
        })
    }
}

fn completion_builder(
    completion: &Completion,
) -> EndpointBuilder<TestContext, (), CompletionFactory> {
    endpoint_builder(PhaseLog::default(), Arc::new(AtomicUsize::new(0)), false)
        .context_factory(CompletionFactory(completion.clone()))
}

fn assert_completion(
    completion: &Completion,
    response: &ResponseParts,
    outcome: ResponseCompletionOutcome,
) {
    let records = completion.0.lock().unwrap();
    assert_eq!(records.as_slice(), &[(outcome, response.body.clone())]);
    assert_eq!(header(response, "x-completed"), Some("yes"));
}

#[tokio::test]
async fn response_completion_preserves_success_and_partial_success_codes() {
    let completion = Completion::default();
    let endpoint = completion_builder(&completion).build().unwrap();
    let response = post(endpoint, r#"{"query":"{ hello fail }"}"#).await;
    assert_eq!(response.status, 200);
    assert_eq!(json(&response)["data"]["hello"], "ok");
    assert_eq!(code(&response).as_deref(), Some("SYNTHETIC_FAILURE"));
    assert_completion(&completion, &response, ResponseCompletionOutcome::Encoded);
}

#[tokio::test]
async fn response_completion_propagates_encoding_limit_failures() {
    let completion = Completion::default();
    let endpoint = completion_builder(&completion)
        .response_byte_limit(128)
        .build()
        .unwrap();
    let response = post(endpoint, r#"{"query":"{ response_field_01: hello response_field_02: hello response_field_03: hello response_field_04: hello response_field_05: hello }"}"#).await;
    assert_eq!(response.status, 500);
    assert_completion(
        &completion,
        &response,
        ResponseCompletionOutcome::EncodingLimitExceeded,
    );
}

#[tokio::test]
async fn deadline_drops_response_hook_and_passes_it_to_completion_processing() {
    let completion = Completion::default();
    let dropped = Arc::new(AtomicBool::new(false));
    let endpoint = completion_builder(&completion)
        .response_hook(PendingHook(dropped.clone()))
        .inject(|components| {
            components
                .http()
                .response_hook_timeout(Duration::from_millis(5));
        })
        .build()
        .unwrap();
    let response = post(endpoint, r#"{"query":"{ hello }"}"#).await;
    assert_eq!(response.status, 504);
    assert!(dropped.load(Ordering::SeqCst));
    assert_completion(&completion, &response, ResponseCompletionOutcome::TimedOut);
}

#[tokio::test]
async fn response_completion_propagates_the_execution_deadline() {
    let completion = Completion::default();
    let dropped = Arc::new(AtomicBool::new(false));
    let executable = schema_builder_with_pending(
        PhaseLog::default(),
        Arc::new(AtomicUsize::new(0)),
        false,
        Some(dropped.clone()),
    )
    .build()
    .unwrap();
    let endpoint = Endpoint::builder(executable)
        .context_factory(CompletionFactory(completion.clone()))
        .inject(|components| {
            components
                .http()
                .execution_timeout(Duration::from_millis(5));
        })
        .build()
        .unwrap();
    let response = post(endpoint, r#"{"query":"{ hello }"}"#).await;
    assert_eq!(response.status, 504);
    assert!(dropped.load(Ordering::SeqCst));
    assert_completion(&completion, &response, ResponseCompletionOutcome::TimedOut);
}

struct FailureHook;

impl chigiri_http::ResponseHook<()> for FailureHook {
    fn call<'a>(
        &'a self,
        _: chigiri_http::ResponseHookContext<'a, ()>,
    ) -> BoxFuture<'a, Result<chigiri_http::ResponseHeaders, HookError>> {
        Box::pin(async {
            Err(HookError::new(std::io::Error::other(
                "private-completion-test",
            )))
        })
    }
}

#[tokio::test]
async fn failure_paths_pass_response_hooks_to_completion_processing() {
    let completion = Completion::default();
    let endpoint = completion_builder(&completion)
        .response_hook(FailureHook)
        .build()
        .unwrap();
    let response = post(endpoint, r#"{"query":"{ hello }"}"#).await;
    assert_eq!(response.status, 500);
    assert!(!String::from_utf8_lossy(&response.body).contains("private-completion-test"));
    assert_completion(&completion, &response, ResponseCompletionOutcome::Failed);
}

struct WaitingHook(Arc<tokio::sync::Notify>, Arc<AtomicBool>);

impl chigiri_http::ResponseHook<()> for WaitingHook {
    fn call<'a>(
        &'a self,
        _: chigiri_http::ResponseHookContext<'a, ()>,
    ) -> BoxFuture<'a, Result<chigiri_http::ResponseHeaders, HookError>> {
        self.0.notify_one();
        Box::pin(Pending::new(self.1.clone()))
    }
}

#[tokio::test]
async fn future_cancellation_does_not_call_for_completion_processing() {
    let completion = Completion::default();
    let entered = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(AtomicBool::new(false));
    let endpoint = completion_builder(&completion)
        .response_hook(WaitingHook(entered.clone(), dropped.clone()))
        .build()
        .unwrap();
    let request = tokio::spawn(async move { post(endpoint, r#"{"query":"{ hello }"}"#).await });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    assert!(dropped.load(Ordering::SeqCst));
    assert!(completion.0.lock().unwrap().is_empty());
}
