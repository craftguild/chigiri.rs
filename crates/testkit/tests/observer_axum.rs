//! Verifies observer-event ordering and terminal results through Axum.

use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use chigiri::{
    EndpointBuilder, LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult, TraceId,
};
use chigiri_http::{BoxFuture, ContextFactory, PrepareError, PreparedContext, TransportAdmission};
use chigiri_observability::DeterministicRequestIdFactory;

use crate::lifecycle_support::{
    PendingTransport, PhaseLog, RecordingTransport, TestContext, endpoint_builder,
};

type Event = (LifecyclePhase, LifecycleResult, &'static str);

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<Event>>>);

impl Recorder {
    fn snapshot(&self) -> Vec<Event> {
        self.0.lock().unwrap().clone()
    }
}

impl LifecycleObserver for Recorder {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.0
            .lock()
            .unwrap()
            .push((event.phase(), event.result(), event.component()));
    }
}

#[derive(Clone, Default)]
struct IdentityRecorder(Arc<Mutex<Vec<(LifecyclePhase, String, String)>>>);

impl LifecycleObserver for IdentityRecorder {
    fn observe(&self, event: LifecycleEvent<'_>) {
        let identity = event.identity();
        self.0.lock().unwrap().push((
            event.phase(),
            identity
                .request_id()
                .map(|id| id.as_str().to_owned())
                .unwrap_or_default(),
            identity
                .trace_id()
                .map(|id| id.as_str().to_owned())
                .unwrap_or_default(),
        ));
    }
}

struct Factory(PhaseLog);

impl ContextFactory<TestContext, ()> for Factory {
    fn create<'a>(
        &'a self,
        _: chigiri_http::RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<TestContext, ()>, PrepareError>> {
        let log = self.0.clone();
        Box::pin(async move { Ok(PreparedContext::execution_only(TestContext { log })) })
    }
}

#[tokio::test]
async fn obs_01_success_closes_the_whole_phase_in_a_fixed_order() {
    let recorder = Recorder::default();
    let log = PhaseLog::default();
    let handler = configured(recorder.clone(), log);
    let response = handler.handle(post()).await;
    assert_eq!(response.status(), StatusCode::OK);

    let events = recorder.snapshot();
    let expected = [
        LifecyclePhase::RequestStarted,
        LifecyclePhase::HttpPreflight,
        LifecyclePhase::TransportAdmission,
        LifecyclePhase::BodyRead,
        LifecyclePhase::HttpDecode,
        LifecyclePhase::OperationSourceResolution,
        LifecyclePhase::DecodedRequestHook,
        LifecyclePhase::ContextConstruction,
        LifecyclePhase::RequestPlugin,
        LifecyclePhase::DocumentPreparation,
        LifecyclePhase::OperationAdmission,
        LifecyclePhase::Execution,
        LifecyclePhase::ResponseHook,
        LifecyclePhase::HttpEncode,
    ];
    let mut cursor = 0;
    for phase in expected {
        let relative = events[cursor..]
            .iter()
            .position(|event| *event == (phase, LifecycleResult::Started, component(phase)))
            .unwrap_or_else(|| panic!("expected the phase to start."));
        cursor += relative + 1;
        if phase != LifecyclePhase::RequestStarted {
            assert!(has_terminal_after(&events, cursor - 1, phase));
        }
    }
    assert!(events.contains(&(
        LifecyclePhase::ServerBuilt,
        LifecycleResult::Succeeded,
        "chigiri"
    )));
    assert_eq!(
        events.last(),
        Some(&(
            LifecyclePhase::RequestCompleted,
            LifecycleResult::Succeeded,
            "http-endpoint"
        ))
    );
}

#[tokio::test]
async fn obs_01_rejection_timeout_and_cancel_close_phases_with_matching_results() {
    let reject = Recorder::default();
    let log = PhaseLog::default();
    assert_eq!(
        configured(reject.clone(), log.clone())
            .with_transport(RecordingTransport { log, reject: true })
            .handle(post())
            .await
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_terminal(&reject, LifecycleResult::Rejected);

    let timeout = Recorder::default();
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handler = configured(timeout.clone(), PhaseLog::default())
        .with_transport(PendingTransport(Arc::clone(&dropped)))
        .with_transport_timeout(Duration::from_millis(5));
    assert_eq!(
        handler.handle(post()).await.status(),
        StatusCode::GATEWAY_TIMEOUT
    );
    assert_terminal(&timeout, LifecycleResult::TimedOut);

    let cancelled = Recorder::default();
    let handler = configured(cancelled.clone(), PhaseLog::default()).with_transport(
        PendingTransport(Arc::new(std::sync::atomic::AtomicBool::new(false))),
    );
    let mut future = Box::pin(handler.handle(post()));
    assert!(matches!(futures_util::poll!(&mut future), Poll::Pending));
    drop(future);
    assert_terminal(&cancelled, LifecycleResult::Cancelled);
}

#[tokio::test]
async fn obs_05_axum_extensions_propagate_trace_and_generated_request_ids_to_all_phases() {
    let recorder = IdentityRecorder::default();
    let log = PhaseLog::default();
    let handler = ObservedHandler(
        endpoint_builder(log.clone(), Arc::new(AtomicUsize::new(0)), false)
            .inject(|components| {
                components
                    .observability()
                    .request_id_factory(DeterministicRequestIdFactory::new(7))
                    .lifecycle_observer(recorder.clone());
            })
            .context_factory(Factory(log)),
    );
    let mut request = post();
    request
        .extensions_mut()
        .insert(TraceId::new("upstream-trace-1").unwrap());

    assert_eq!(handler.handle(request).await.status(), StatusCode::OK);
    let records = recorder.0.lock().unwrap();
    let request_records = records
        .iter()
        .filter(|record| record.0 != LifecyclePhase::ServerBuilt)
        .collect::<Vec<_>>();
    assert!(!request_records.is_empty());
    assert!(
        request_records
            .iter()
            .all(|record| record.1 == "request-0000000000000007")
    );
    assert!(
        request_records
            .iter()
            .all(|record| record.2 == "upstream-trace-1")
    );
}

fn configured(recorder: Recorder, log: PhaseLog) -> ObservedHandler {
    ObservedHandler(
        endpoint_builder(log.clone(), Arc::new(AtomicUsize::new(0)), false)
            .inject(|components| {
                components.observability().lifecycle_observer(recorder);
            })
            .context_factory(Factory(log)),
    )
}

struct ObservedHandler(EndpointBuilder<TestContext, (), Factory>);

impl ObservedHandler {
    fn with_transport(mut self, admission: impl TransportAdmission) -> Self {
        self.0 = self.0.inject(|components| {
            components.http().push_transport_admission(admission);
        });
        self
    }

    fn with_transport_timeout(mut self, timeout: Duration) -> Self {
        self.0 = self.0.inject(|components| {
            components.http().transport_admission_timeout(timeout);
        });
        self
    }

    async fn handle(self, request: Request) -> axum::response::Response {
        chigiri_axum::handler(self.0.build().unwrap())(request).await
    }
}

fn post() -> Request {
    Request::builder()
        .method(Method::POST)
        .uri("/graphql")
        .header("content-type", "application/json")
        .header("accept", "application/graphql-response+json")
        .body(Body::from(r#"{"query":"{ hello }"}"#))
        .unwrap()
}

fn assert_terminal(recorder: &Recorder, result: LifecycleResult) {
    let events = recorder.snapshot();
    assert!(events.contains(&(LifecyclePhase::TransportAdmission, result, "http-endpoint")));
    assert!(events.contains(&(LifecyclePhase::RequestCompleted, result, "http-endpoint")));
}

fn has_terminal_after(events: &[Event], started: usize, phase: LifecyclePhase) -> bool {
    events[started + 1..].iter().any(|event| {
        event.0 == phase
            && matches!(
                event.1,
                LifecycleResult::Succeeded
                    | LifecycleResult::Rejected
                    | LifecycleResult::Failed
                    | LifecycleResult::TimedOut
                    | LifecycleResult::Cancelled
            )
    })
}

fn component(phase: LifecyclePhase) -> &'static str {
    match phase {
        LifecyclePhase::RequestPlugin
        | LifecyclePhase::DocumentPreparation
        | LifecyclePhase::OperationAdmission
        | LifecyclePhase::Execution => "chigiri",
        _ => "http-endpoint",
    }
}
