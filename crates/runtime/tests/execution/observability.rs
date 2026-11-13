//! Test error correlation, observer noninterference, cancellation, and field opt-in.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, ResolvedObject, ResolvedValue, ResolverDispatch,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    ErrorEvent, ErrorId, ErrorIdFactory, ErrorIdFactoryError, ErrorObserver, ExecutableSchema,
    FieldError, FieldResult, GraphQlRequest, LifecycleEvent, LifecycleObserver, LifecyclePhase,
    LifecycleResult, RequestId, RequestIdFactory, RequestIdFactoryError, RequestIdentity, Sampler,
    SamplingDecision, Value,
};

use super::{Dispatch, SCHEMA, block_on};

type LifecycleRecord = (LifecyclePhase, LifecycleResult, &'static str);
type ErrorRecord = (Option<String>, String);

#[derive(Clone, Default)]
struct Recorder {
    lifecycle: Arc<Mutex<Vec<LifecycleRecord>>>,
    errors: Arc<Mutex<Vec<ErrorRecord>>>,
}

impl Recorder {
    fn lifecycle(&self) -> Vec<LifecycleRecord> {
        self.lifecycle.lock().unwrap().clone()
    }

    fn errors(&self) -> Vec<ErrorRecord> {
        self.errors.lock().unwrap().clone()
    }
}

impl LifecycleObserver for Recorder {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.lifecycle
            .lock()
            .unwrap()
            .push((event.phase(), event.result(), event.component()));
    }
}

impl ErrorObserver for Recorder {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        self.errors.lock().unwrap().push((
            event.error_id().map(|id| id.as_str().to_owned()),
            event.source().to_string(),
        ));
    }
}

struct SequentialIds(AtomicU64);

impl ErrorIdFactory for SequentialIds {
    fn create(&self) -> Result<ErrorId, ErrorIdFactoryError> {
        let value = self.0.fetch_add(1, Ordering::SeqCst);
        ErrorId::new(format!("failure-{value}")).map_err(ErrorIdFactoryError::new)
    }
}

#[derive(Clone, Default)]
struct CorrelationRecorder(Arc<Mutex<Vec<(LifecyclePhase, String, SamplingDecision)>>>);

impl LifecycleObserver for CorrelationRecorder {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.0.lock().unwrap().push((
            event.phase(),
            event
                .identity()
                .request_id()
                .map(|id| id.as_str().to_owned())
                .unwrap_or_default(),
            event.identity().sampling(),
        ));
    }
}

struct CountingRequestIds(Arc<AtomicU64>);

impl RequestIdFactory for CountingRequestIds {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        let call = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        RequestId::new(format!("request-{call}")).map_err(RequestIdFactoryError::new)
    }
}

struct CountingDropSampler(Arc<AtomicU64>);

impl Sampler for CountingDropSampler {
    fn sample(&self, identity: RequestIdentity<'_>) -> SamplingDecision {
        assert_eq!(
            identity.request_id().map(RequestId::as_str),
            Some("request-1")
        );
        self.0.fetch_add(1, Ordering::SeqCst);
        SamplingDecision::Drop
    }
}

struct InternalDispatch;

impl ResolverDispatch<()> for InternalDispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        super::dispatch_fingerprint()
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        field_name: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        let field = match field_name {
            "requiredError" => "requiredError",
            "nullableError" => "nullableError",
            _ => "other",
        };
        Box::pin(async move { Err(FieldError::internal(SecretFailure(field))) })
    }
}

struct PendingDispatch;

impl ResolverDispatch<()> for PendingDispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        super::dispatch_fingerprint()
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(std::future::pending())
    }
}

struct SecretFailure(&'static str);

impl std::fmt::Debug for SecretFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretFailure")
    }
}

impl std::fmt::Display for SecretFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "database-password=never-log-this;field={}",
            self.0
        )
    }
}

impl std::error::Error for SecretFailure {}

#[test]
fn obs_02_and_03_mask_sources_for_independent_request_and_error_id_failures() {
    let recorder = Recorder::default();
    let schema = ExecutableSchema::builder(&SCHEMA, InternalDispatch)
        .error_id_factory(SequentialIds(AtomicU64::new(1)))
        .error_observer(recorder.clone())
        .build()
        .unwrap();

    let response =
        block_on(schema.execute(GraphQlRequest::new("{ requiredError nullableError }"), ()));
    let ids = response
        .errors
        .iter()
        .map(|error| {
            error
                .extensions
                .get("errorId")
                .and_then(Value::as_str)
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    let observed = recorder.errors();
    for error in &response.errors {
        let id = error
            .extensions
            .get("errorId")
            .and_then(Value::as_str)
            .unwrap();
        let chigiri_runtime::PathSegment::Field(field) = &error.path[0] else {
            panic!("expected the root field path");
        };
        let source = observed
            .iter()
            .find(|(observed_id, _)| observed_id.as_deref() == Some(id))
            .map(|(_, source)| source)
            .unwrap();
        assert!(source.contains(field));
    }
    assert!(
        observed
            .iter()
            .all(|(_, source)| source.contains("never-log"))
    );
    assert!(!format!("{response:?}").contains("never-log"));
    assert!(
        response
            .errors
            .iter()
            .all(|error| error.message == "Internal server error")
    );
}

#[test]
fn obs_04_observer_failures_cannot_change_graphql_semantics() {
    let plain = ExecutableSchema::builder(&SCHEMA, InternalDispatch)
        .build()
        .unwrap();
    let observed = ExecutableSchema::builder(&SCHEMA, InternalDispatch)
        .error_observer(Recorder::default())
        .lifecycle_observer(Recorder::default())
        .build()
        .unwrap();
    let request = GraphQlRequest::new("{ nullableError }");

    assert_eq!(
        block_on(plain.execute(request.clone(), ())),
        block_on(observed.execute(request, ()))
    );
}

#[test]
fn obs_09_field_instrumentation_emits_success_events_only_when_enabled() {
    let disabled = Recorder::default();
    let disabled_schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .lifecycle_observer(disabled.clone())
        .build()
        .unwrap();
    let enabled = Recorder::default();
    let enabled_schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .lifecycle_observer(enabled.clone())
        .field_instrumentation(true)
        .build()
        .unwrap();

    block_on(disabled_schema.execute(GraphQlRequest::new("{ hello }"), ()));
    block_on(enabled_schema.execute(GraphQlRequest::new("{ hello }"), ()));
    assert_eq!(field_successes(&disabled), 0);
    assert_eq!(field_successes(&enabled), 1);
}

#[test]
fn obs_05_request_id_and_sampling_are_fixed_once_at_request_start() {
    let request_calls = Arc::new(AtomicU64::new(0));
    let sample_calls = Arc::new(AtomicU64::new(0));
    let recorder = CorrelationRecorder::default();
    let schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .request_id_factory(CountingRequestIds(Arc::clone(&request_calls)))
        .sampler(CountingDropSampler(Arc::clone(&sample_calls)))
        .lifecycle_observer(recorder.clone())
        .build()
        .unwrap();

    block_on(schema.execute(GraphQlRequest::new("{ hello }"), ()));

    assert_eq!(request_calls.load(Ordering::SeqCst), 1);
    assert_eq!(sample_calls.load(Ordering::SeqCst), 1);
    let records = recorder.0.lock().unwrap();
    let request_records = records
        .iter()
        .filter(|record| record.0 != LifecyclePhase::ServerBuilt)
        .collect::<Vec<_>>();
    assert!(!request_records.is_empty());
    assert!(request_records.iter().all(|record| record.1 == "request-1"));
    assert!(
        request_records
            .iter()
            .all(|record| record.2 == SamplingDecision::Drop)
    );
}

#[test]
fn obs_01_future_destruction_closes_execution_and_request_as_cancelled() {
    let recorder = Recorder::default();
    let schema = ExecutableSchema::builder(&SCHEMA, PendingDispatch)
        .lifecycle_observer(recorder.clone())
        .build()
        .unwrap();
    let mut future = Box::pin(schema.execute(GraphQlRequest::new("{ hello }"), ()));
    assert!(matches!(poll_once(future.as_mut()), Poll::Pending));
    drop(future);

    let events = recorder.lifecycle();
    assert!(events.contains(&(
        LifecyclePhase::Execution,
        LifecycleResult::Cancelled,
        "chigiri"
    )));
    assert!(events.contains(&(
        LifecyclePhase::RequestPlugin,
        LifecycleResult::Cancelled,
        "chigiri"
    )));
    assert!(events.contains(&(
        LifecyclePhase::RequestCompleted,
        LifecycleResult::Cancelled,
        "direct"
    )));
    let terminal_order = events
        .iter()
        .filter(|(_, result, _)| *result == LifecycleResult::Cancelled)
        .map(|(phase, _, _)| *phase)
        .collect::<Vec<_>>();
    assert_eq!(
        terminal_order,
        [
            LifecyclePhase::Execution,
            LifecyclePhase::RequestPlugin,
            LifecyclePhase::RequestCompleted,
        ]
    );
}

fn field_successes(recorder: &Recorder) -> usize {
    recorder
        .lifecycle()
        .into_iter()
        .filter(|(phase, result, component)| {
            *phase == LifecyclePhase::Execution
                && *result == LifecycleResult::Succeeded
                && *component == "field"
        })
        .count()
}

fn poll_once<F: Future>(future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}
