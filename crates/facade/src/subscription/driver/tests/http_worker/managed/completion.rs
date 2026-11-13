//! Managed initial failure preserves outcome, observation and physical ownership.

mod poison;

use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};

use chigiri_http::{
    ContextFactory, PrepareError, PreparedContext, RequestContext, ResponseCompletion,
    ResponseCompletionOutcome, ResponseParts,
};
use chigiri_runtime::{
    ErrorEvent, ErrorObserver, ExecutableSchema, LifecycleEvent, LifecycleObserver, LifecyclePhase,
    LifecycleResult,
};

use super::super::super::descriptor::SCHEMA;
use super::super::super::dispatch::Dispatch;
use super::super::super::harness::{Factory, Hook};
use super::super::super::state::HookState;
use super::*;

struct Release(Option<SyncSender<()>>);
impl Release {
    fn send(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.send();
    }
}

struct Signals {
    entered: AtomicUsize,
    errors: AtomicUsize,
    release: Mutex<Receiver<()>>,
    observer_panics: bool,
}

#[derive(Clone)]
struct Observe(Arc<Signals>);
impl LifecycleObserver for Observe {
    fn observe(&self, event: LifecycleEvent<'_>) {
        if event.phase() == LifecyclePhase::Execution
            && event.result() == LifecycleResult::Succeeded
        {
            assert_eq!(self.0.entered.fetch_add(1, Ordering::SeqCst), 0);
            self.0
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(2))
                .expect("owned source boundary release");
        }
    }
}
impl ErrorObserver for Observe {
    fn observe_error(&self, _: ErrorEvent<'_>) {
        self.0.errors.fetch_add(1, Ordering::SeqCst);
        if self.0.observer_panics {
            panic!("private subscription observer failure");
        }
    }
}

#[derive(Clone, Copy)]
enum FailureMode {
    Panic,
    Payload,
    Token,
    Both,
    Observer,
}

#[derive(Clone, Copy)]
enum ShutdownOrder {
    StopBeforeCompletion,
    CompleteBeforeStop,
}

struct CompletionFactory(Factory, FailureMode);
impl ContextFactory<AppContext, HookState> for CompletionFactory {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, HookState>, PrepareError>> {
        Box::pin(async move {
            let (context, state, completion) = self.0.create(request).await?.__into_parts();
            Ok(PreparedContext::new(context, state)
                .with_response_completion(FailingCompletion(completion.unwrap(), self.1)))
        })
    }
}
struct FailingCompletion(Arc<dyn ResponseCompletion>, FailureMode);
impl ResponseCompletion for FailingCompletion {
    fn complete(
        &self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        let response = self.0.complete(response, outcome);
        if matches!(self.1, FailureMode::Token) {
            return response;
        }
        drop(response);
        if matches!(self.1, FailureMode::Payload) {
            std::panic::panic_any(Payload);
        }
        panic!("private abandoned subscription completion failure");
    }
}

impl Drop for FailingCompletion {
    fn drop(&mut self) {
        if matches!(self.1, FailureMode::Token | FailureMode::Both) {
            panic!("private completion token destruction failure");
        }
    }
}
struct Payload;
impl Drop for Payload {
    fn drop(&mut self) {
        panic!("private completion panic-payload destruction failure");
    }
}

fn fixture(probe: Arc<State>, mode: FailureMode) -> (Endpoint<AppContext>, Arc<Signals>, Release) {
    let (release, released) = sync_channel(1);
    let signals = Arc::new(Signals {
        entered: AtomicUsize::new(0),
        errors: AtomicUsize::new(0),
        release: Mutex::new(released),
        observer_panics: matches!(mode, FailureMode::Observer),
    });
    let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
        .build()
        .unwrap();
    let endpoint = Endpoint::builder(schema)
        .context_factory(CompletionFactory(Factory(probe.clone()), mode))
        .response_hook(Hook(probe))
        .subscription_limits(SubscriptionLimits {
            max_active_subscriptions: 1,
            ..SubscriptionLimits::default()
        })
        .inject(|components| {
            components
                .observability()
                .lifecycle_observer(Observe(signals.clone()))
                .error_observer(Observe(signals.clone()));
        })
        .build()
        .unwrap();
    (endpoint, signals, Release(Some(release)))
}

pub(super) fn invoke(
    endpoint: &Endpoint<AppContext>,
    service: &ManagedService,
    profile: SubscriptionTransport,
) -> BoxFuture<'static, ManagedHttpResponse> {
    let accept = match profile {
        SubscriptionTransport::ServerSentEvents => "text/event-stream",
        SubscriptionTransport::Multipart => "multipart/mixed; subscriptionSpec=1.0",
        _ => unreachable!(),
    };
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", accept.as_bytes()),
        RequestHeader::new("authorization", b"original-secret"),
    ];
    let metadata = ApplicationMetadata::default();
    endpoint.__handle_managed_http(
        service.request().unwrap(),
        TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata),
        BufferedBody::new(VALID.to_vec()),
        WebSocketHttpVersion::Http11,
        &TIMERS,
    )
}

async fn fail_initial(
    endpoint: &Endpoint<AppContext>,
    service: &ManagedService,
    profile: SubscriptionTransport,
    signals: &Signals,
    probe: &State,
    release: &mut Release,
    order: ShutdownOrder,
) {
    let response = invoke(endpoint, service, profile);
    until(&signals.entered, 1).await;
    let mut shutdown = if matches!(order, ShutdownOrder::StopBeforeCompletion) {
        let mut hook = endpoint.shutdown_on(async {}, ShutdownOptions::graceful());
        assert!(
            poll_fn(|cx| std::task::Poll::Ready(hook.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        Some(hook)
    } else {
        None
    };
    assert_eq!(count(&probe.counts.completion_calls), 0);
    // Stop precedes the child result; release this synchronous callback before
    // waiting for another task, including on a runtime with two worker threads.
    release.send();
    let response = response.await;
    match order {
        ShutdownOrder::StopBeforeCompletion => assert!(matches!(
            response,
            ManagedHttpResponse::Empty { status: 500 | 503 }
        )),
        ShutdownOrder::CompleteBeforeStop => assert!(matches!(
            response,
            ManagedHttpResponse::Empty { status: 500 }
        )),
    }
    shutdown
        .take()
        .unwrap_or_else(|| endpoint.shutdown_on(async {}, ShutdownOptions::graceful()))
        .await;
    until(&probe.counts.completion_drops, 1).await;
    until(&signals.errors, 1).await;
    assert_eq!(
        endpoint.shutdown_outcome(),
        Some(ShutdownOutcome::InfrastructureFailure)
    );
    assert_eq!(count(&signals.errors), 1);
    assert_eq!(count(&probe.counts.completion_calls), 1);
    assert_eq!(count(&probe.counts.source_drops), 1);
    let expected_hooks = usize::from(matches!(order, ShutdownOrder::CompleteBeforeStop));
    assert_eq!(count(&probe.counts.hook_calls), expected_hooks);
    assert!(service.request().is_err());
}

fn completion_failure(order: ShutdownOrder) {
    let runtime = runtime();
    for profile in profiles() {
        for mode in [FailureMode::Panic, FailureMode::Observer] {
            let probe = State::new(vec![SourceStep::Pending]);
            let (endpoint, signals, mut release) = fixture(probe.clone(), mode);
            run(&runtime, async {
                let service = endpoint.__managed_service().unwrap();
                fail_initial(
                    &endpoint,
                    &service,
                    profile,
                    &signals,
                    &probe,
                    &mut release,
                    order,
                )
                .await;
                let control = endpoint.managed_control();
                tokio::time::timeout(Duration::from_secs(2), async {
                    while crate::subscription::worker::managed_supervisor_retained_for_test(
                        &control,
                    ) {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("failed SDK supervisor must physically join");
                assert_eq!(endpoint.subscription_physical_counts_for_test(), (0, 0));
            });
        }
    }
}

#[test]
fn abandoned_preparation_completion_failure_cannot_be_reported_as_drained() {
    completion_failure(ShutdownOrder::StopBeforeCompletion);
}

#[test]
fn ordinary_preparation_completion_failure_returns_safe_500_and_one_observation() {
    completion_failure(ShutdownOrder::CompleteBeforeStop);
}
