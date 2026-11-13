//! Public generated consumers register instrumentation independently of observers.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chigiri::spi::{ApplicationMetadata, RequestHeader};
use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::noop::NoopTracer;
use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState};

use super::support::{resolvers, schema_builder};

#[test]
fn one_public_registration_slot_replaces_in_call_order_and_preserves_observers() {
    let first = Arc::new(OpenTelemetryInstrumentation::new(NoopTracer::new()));
    let last = Arc::new(OpenTelemetryInstrumentation::new(NoopTracer::new()));
    let calls = Counts::default();
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components
                .observability()
                .instrumentation(Arc::clone(&first))
                .lifecycle_observer(calls.clone())
                .error_observer(calls.clone())
                .request_id_factory(FirstIdentityOnly(AtomicUsize::new(0)));
        })
        .inject(|components| {
            components
                .observability()
                .instrumentation(Arc::clone(&last));
        })
        .without_context()
        .build()
        .unwrap();
    assert_eq!(Arc::strong_count(&first), 1);
    assert_eq!(Arc::strong_count(&last), 2);
    assert_eq!(calls.lifecycle.load(Ordering::SeqCst), 1);
    assert!(endpoint.request_telemetry(None).request_id().is_some());
    assert!(endpoint.request_telemetry(None).request_id().is_none());
    assert_eq!(calls.errors.load(Ordering::SeqCst), 1);
    drop(endpoint);
    assert_eq!(Arc::strong_count(&last), 1);
}

#[test]
fn built_schema_input_uses_only_the_endpoint_instrumentation_slot() {
    let old = Arc::new(OpenTelemetryInstrumentation::new(NoopTracer::new()));
    let new = Arc::new(OpenTelemetryInstrumentation::new(NoopTracer::new()));
    let source = schema_builder()
        .instrumentation(Arc::clone(&old))
        .build()
        .unwrap();
    let endpoint = chigiri::Endpoint::builder(source)
        .inject(|components| {
            components.observability().instrumentation(Arc::clone(&new));
        })
        .without_context()
        .build()
        .unwrap();
    assert_eq!(Arc::strong_count(&old), 1);
    assert_eq!(Arc::strong_count(&new), 2);
    drop(endpoint);
    assert_eq!(Arc::strong_count(&new), 1);
}

#[test]
fn shared_entry_capture_keeps_suppression_and_parent_when_worker_identity_creation_fails() {
    let calls = Counts::default();
    let sampled = Arc::new(Mutex::new(Vec::new()));
    let schema = schema_builder()
        .instrumentation(OpenTelemetryInstrumentation::new(NoopTracer::new()))
        .request_id_factory(FirstIdentityOnly(AtomicUsize::new(0)))
        .error_observer(calls.clone())
        .sampler(ObserveSampling(Arc::clone(&sampled)))
        .build()
        .unwrap();
    assert!(schema.request_telemetry(None).request_id().is_some());
    let headers = [
        RequestHeader::new(
            "traceparent",
            b"00-0123456789abcdef0123456789abcdef-0123456789abcdef-01",
        ),
        RequestHeader::new("authorization", b"private-header"),
        RequestHeader::new("baggage", b"private-baggage"),
    ];
    let metadata = ApplicationMetadata::default();
    let request =
        chigiri::TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let private = Arc::new(());
    let weak = Arc::downgrade(&private);
    let captured = {
        let current = Context::new().with_value(PrivateValue(private));
        assert_eq!(
            Arc::strong_count(&current.get::<PrivateValue>().unwrap().0),
            1
        );
        let _guard = current.with_telemetry_suppressed().attach();
        schema.capture_instrumentation(&request).unwrap()
    };
    assert!(weak.upgrade().is_none());
    assert!(captured.values().is_suppressed());
    let trace = std::thread::spawn(move || {
        let worker = SpanContext::new(
            TraceId::from_bytes([9; 16]),
            SpanId::from_bytes([9; 8]),
            TraceFlags::SAMPLED,
            false,
            TraceState::default(),
        );
        let _guard = Context::new()
            .with_remote_span_context(worker.clone())
            .attach();
        let telemetry = schema.request_telemetry_with_capture(
            Some(chigiri::TraceId::new("opaque-correlation").unwrap()),
            Some(&captured),
            chigiri::spi::InstrumentationKind::Request,
        );
        assert!(telemetry.request_id().is_none());
        telemetry.instrumentation_scope().unwrap().run(|| {
            let current = Context::current();
            assert!(current.is_telemetry_suppressed());
            assert!(current.get::<PrivateValue>().is_none());
            assert_eq!(
                current.span().span_context().trace_id().to_string(),
                "0123456789abcdef0123456789abcdef"
            );
        });
        assert_eq!(*Context::current().span().span_context(), worker);
        assert!(!Context::current().is_telemetry_suppressed());
        telemetry.trace_id().unwrap().as_str().to_owned()
    })
    .join()
    .unwrap();
    assert_eq!(trace, "0123456789abcdef0123456789abcdef");
    assert_eq!(calls.errors.load(Ordering::SeqCst), 1);
    assert_eq!(
        sampled.lock().unwrap().as_slice(),
        [(false, false), (true, true)]
    );
}

#[derive(Clone)]
struct PrivateValue(Arc<()>);

struct ObserveSampling(Arc<Mutex<Vec<(bool, bool)>>>);

impl chigiri::Sampler for ObserveSampling {
    fn sample(&self, _: chigiri::RequestIdentity<'_>) -> chigiri::SamplingDecision {
        let current = Context::current();
        self.0.lock().unwrap().push((
            current.is_telemetry_suppressed(),
            current.span().span_context().is_valid(),
        ));
        chigiri::SamplingDecision::Record
    }
}

#[derive(Clone, Default)]
struct Counts {
    lifecycle: Arc<AtomicUsize>,
    errors: Arc<AtomicUsize>,
}

impl chigiri::LifecycleObserver for Counts {
    fn observe(&self, _: chigiri::LifecycleEvent<'_>) {
        self.lifecycle.fetch_add(1, Ordering::SeqCst);
    }
}

impl chigiri::ErrorObserver for Counts {
    fn observe_error(&self, _: chigiri::ErrorEvent<'_>) {
        self.errors.fetch_add(1, Ordering::SeqCst);
    }
}

struct FirstIdentityOnly(AtomicUsize);

impl chigiri::RequestIdFactory for FirstIdentityOnly {
    fn create(&self) -> Result<chigiri::RequestId, chigiri::RequestIdFactoryError> {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            chigiri::RequestId::new("registration-test")
                .map_err(chigiri::RequestIdFactoryError::new)
        } else {
            Err(chigiri::RequestIdFactoryError::new(std::io::Error::other(
                "identity deliberately unavailable",
            )))
        }
    }
}
