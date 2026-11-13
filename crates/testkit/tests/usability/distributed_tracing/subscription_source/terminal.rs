//! Source errors retain subscriber control through observation and owned disposal.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chigiri::spi::InstrumentationKind;
use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, TraceContextExt};

use super::super::super::support::{block_on, schema_builder};
use super::{FixedSampling, PrivateValue, fixture, parent, recording, suppression};

#[test]
fn source_terminal_observer_and_error_destruction_use_subscriber_context() {
    for suppressed in [false, true] {
        for has_parent in [false, true] {
            for retain_parent in [false, true] {
                for with_metadata in [false, true] {
                    for defer_cleanup in [false, true] {
                        for decision in [
                            chigiri::SamplingDecision::Record,
                            chigiri::SamplingDecision::Drop,
                        ] {
                            check(
                                suppressed,
                                has_parent,
                                retain_parent,
                                with_metadata,
                                defer_cleanup,
                                decision,
                            );
                        }
                    }
                }
            }
        }
    }
}

fn check(
    suppressed: bool,
    has_parent: bool,
    retain_parent: bool,
    with_metadata: bool,
    defer_cleanup: bool,
    decision: chigiri::SamplingDecision,
) {
    let (_provider, tracer, records) = recording::provider();
    let calls = Arc::clone(&tracer.calls);
    let bootstrap = schema_builder()
        .instrumentation(OpenTelemetryInstrumentation::new(tracer.clone()))
        .sampler(FixedSampling(decision))
        .build()
        .unwrap();
    let captured = {
        let incoming = if has_parent {
            Context::new().with_remote_span_context(parent(7))
        } else {
            Context::new()
        };
        let _guard = suppression(incoming, suppressed).attach();
        bootstrap.capture_instrumentation(&()).unwrap()
    };
    let telemetry = bootstrap.request_telemetry_with_capture(
        None,
        Some(&captured),
        InstrumentationKind::SubscriptionSetup,
    );
    let setup = telemetry.instrumentation_scope().unwrap().clone();
    let expected = if retain_parent {
        setup.run(|| Context::current().span().span_context().clone())
    } else {
        SpanContext::empty_context()
    };
    let probe = Arc::new(fixture::Probe::new(expected.clone(), suppressed, true));
    let schema = fixture::builder(Arc::clone(&probe))
        .instrumentation(OpenTelemetryInstrumentation::new(tracer))
        .sampler(FixedSampling(decision))
        .error_observer(fixture::Errors(Arc::clone(&probe)))
        .build()
        .unwrap();
    let request = chigiri::GraphQlRequest::new("subscription { ticks { value } }")
        .with_request_telemetry(telemetry);
    let mut prepared = setup.run(|| schema.prepare_subscription(request).unwrap());
    prepared
        .retain_instrumentation(if retain_parent { usize::MAX } else { 0 })
        .unwrap();
    let retained = prepared.retained_request();
    assert!(
        retained
            .request_telemetry()
            .unwrap()
            .instrumentation_scope()
            .is_none()
    );
    let mut session = match block_on(prepared.start(&(), None)) {
        chigiri::__private::SubscriptionStart::Ready(session) => *session,
        chigiri::__private::SubscriptionStart::Response(response) => panic!("{response:?}"),
    };
    if defer_cleanup {
        session.defer_source_cleanup();
    }
    setup.finish(chigiri::LifecycleResult::Succeeded);
    drop(setup);
    let started = calls.load(Ordering::SeqCst);
    assert_eq!(records.take().len(), started);
    let next = session.next_source(with_metadata.then(|| retained.request_telemetry().unwrap()));
    let response = std::thread::scope(|threads| {
        threads
            .spawn(|| {
                let worker = suppression(
                    Context::new()
                        .with_remote_span_context(parent(9))
                        .with_value(PrivateValue),
                    !suppressed,
                );
                let _guard = worker.attach();
                let result = match block_on(next) {
                    chigiri::__private::SubscriptionSourceState::Terminal(response) => {
                        response.into_response()
                    }
                    _ => panic!("one source failure must terminate"),
                };
                assert_eq!(*Context::current().span().span_context(), parent(9));
                assert_eq!(Context::current().is_telemetry_suppressed(), !suppressed);
                assert!(Context::current().get::<PrivateValue>().is_some());
                result
            })
            .join()
            .unwrap()
    });
    assert!(response.data.is_none());
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(probe.errors.lock().unwrap().as_slice(), [probe.expected()]);
    assert_eq!(
        probe.error_drops.lock().unwrap().as_slice(),
        [probe.expected()]
    );
    assert_eq!(probe.polls.load(Ordering::SeqCst), 2);
    assert_eq!(
        probe.drops.load(Ordering::SeqCst),
        usize::from(!defer_cleanup)
    );
    assert_eq!(probe.event_drops.load(Ordering::SeqCst), 0);
    let ended = records.take();
    let error_recorded = !suppressed && expected.is_valid();
    assert_eq!(ended.len(), usize::from(error_recorded));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        started + usize::from(error_recorded)
    );
    if let Some(error) = ended.first() {
        assert_eq!(error.name, "chigiri.error");
        assert_eq!(error.parent_span_id, expected.span_id());
        assert_eq!(error.span_context.trace_id(), expected.trace_id());
        assert!(!format!("{:?}", error.attributes).contains("private subscription-source detail"));
    }
    let _guard = suppression(
        Context::new()
            .with_remote_span_context(parent(9))
            .with_value(PrivateValue),
        !suppressed,
    )
    .attach();
    drop(session);
    assert_eq!(probe.drops.load(Ordering::SeqCst), 1);
    assert_eq!(*Context::current().span().span_context(), parent(9));
    assert_eq!(Context::current().is_telemetry_suppressed(), !suppressed);
}
