//! Detached source activation and fresh event metadata use only selected values.

#[path = "subscription_source/fixture.rs"]
mod fixture;
#[path = "subscription_source/terminal.rs"]
mod terminal;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context as TaskContext, Waker};

use chigiri::spi::InstrumentationKind;
use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, TraceContextExt};

use super::super::support::{block_on, schema_builder};
use super::{FixedSampling, PrivateValue, parent, recording};

#[test]
fn source_waiting_and_disposal_use_detached_subscriber_control_after_setup_ends() {
    for suppressed in [false, true] {
        for has_parent in [false, true] {
            for retain_parent in [false, true] {
                for polls in [0, 1, 2] {
                    let (_provider, tracer, records) = recording::provider();
                    let calls = Arc::clone(&tracer.calls);
                    let bootstrap = schema_builder()
                        .instrumentation(OpenTelemetryInstrumentation::new(tracer.clone()))
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
                    let probe = Arc::new(fixture::Probe::new(expected, suppressed, false));
                    let schema = fixture::builder(Arc::clone(&probe))
                        .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                        .build()
                        .unwrap();
                    let request = chigiri::GraphQlRequest::new("subscription { ticks { value } }")
                        .with_request_telemetry(telemetry);
                    let mut prepared = setup.run(|| schema.prepare_subscription(request).unwrap());
                    let required = prepared.required_retained_metadata_bytes().unwrap();
                    prepared
                        .retain_instrumentation(if retain_parent { usize::MAX } else { 0 })
                        .unwrap();
                    let retained = prepared.retained_request();
                    let metadata = retained.request_telemetry().unwrap();
                    assert!(metadata.instrumentation_scope().is_none());
                    assert_eq!(
                        metadata.propagation_context().unwrap().is_suppressed(),
                        suppressed
                    );
                    if !retain_parent {
                        assert!(metadata.trace_id().is_none());
                        assert_eq!(prepared.retained_metadata_bytes(), Some(required));
                    }
                    drop(retained);
                    let mut session = match block_on(prepared.start(&(), None)) {
                        chigiri::__private::SubscriptionStart::Ready(session) => *session,
                        chigiri::__private::SubscriptionStart::Response(response) => {
                            panic!("{response:?}")
                        }
                    };
                    setup.finish(chigiri::LifecycleResult::Succeeded);
                    drop(setup);
                    let started = calls.load(Ordering::SeqCst);
                    assert_eq!(records.take().len(), started);
                    let mut next = session.next_source(None);
                    let worker = |byte| {
                        suppression(
                            Context::new()
                                .with_remote_span_context(parent(byte))
                                .with_value(PrivateValue),
                            !suppressed,
                        )
                    };
                    if polls > 0 {
                        let _guard = worker(8).attach();
                        let mut task = TaskContext::from_waker(Waker::noop());
                        assert!(next.as_mut().poll(&mut task).is_pending());
                        assert_eq!(*Context::current().span().span_context(), parent(8));
                        assert_eq!(Context::current().is_telemetry_suppressed(), !suppressed);
                    }
                    std::thread::scope(|threads| {
                        threads
                            .spawn(|| {
                                let _guard = worker(9).attach();
                                if polls == 2 {
                                    assert!(matches!(
                                        block_on(next),
                                        chigiri::__private::SubscriptionSourceState::EventReady
                                    ));
                                } else {
                                    drop(next);
                                }
                                assert_eq!(*Context::current().span().span_context(), parent(9));
                                assert_eq!(
                                    Context::current().is_telemetry_suppressed(),
                                    !suppressed
                                );
                            })
                            .join()
                            .unwrap()
                    });
                    std::thread::scope(|threads| {
                        threads
                            .spawn(|| {
                                let _guard = worker(9).attach();
                                drop(session);
                                assert_eq!(*Context::current().span().span_context(), parent(9));
                                assert_eq!(
                                    Context::current().is_telemetry_suppressed(),
                                    !suppressed
                                );
                            })
                            .join()
                            .unwrap();
                    });
                    assert_eq!(probe.polls.load(Ordering::SeqCst), polls);
                    assert_eq!(probe.drops.load(Ordering::SeqCst), 1);
                    assert_eq!(
                        probe.event_drops.load(Ordering::SeqCst),
                        usize::from(polls == 2)
                    );
                    assert_eq!(calls.load(Ordering::SeqCst), started);
                    assert!(records.take().is_empty());
                    if suppressed {
                        assert_eq!(started, 0);
                    }
                }
            }
        }
    }
}

#[test]
fn event_metadata_never_copies_setup_or_opaque_trace_identity() {
    for suppressed in [false, true] {
        for decision in [
            chigiri::SamplingDecision::Record,
            chigiri::SamplingDecision::Drop,
        ] {
            for has_parent in [false, true] {
                let (_provider, tracer, records) = recording::provider();
                let calls = Arc::clone(&tracer.calls);
                let schema = schema_builder()
                    .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                    .sampler(FixedSampling(decision))
                    .build()
                    .unwrap();
                let values = chigiri::spi::PropagationContext::new(
                    has_parent.then(|| {
                        chigiri::spi::TraceParent::new([7; 16], [7; 8], 1, true, "").unwrap()
                    }),
                    suppressed,
                );
                let captured = schema.instrumentation_with_values(values).unwrap();
                let worker = suppression(
                    Context::new()
                        .with_remote_span_context(parent(9))
                        .with_value(PrivateValue),
                    !suppressed,
                );
                let _guard = worker.attach();
                let mut roots = Vec::new();
                for _ in 0..2 {
                    let telemetry = schema.request_telemetry_with_capture(
                        Some(chigiri::TraceId::new("legacy-setup").unwrap()),
                        Some(&captured),
                        InstrumentationKind::SubscriptionEvent,
                    );
                    let scope = telemetry.instrumentation_scope().unwrap();
                    scope.run(|| {
                        let current = Context::current();
                        let span = current.span().span_context().clone();
                        assert_eq!(current.is_telemetry_suppressed(), suppressed);
                        assert!(current.get::<PrivateValue>().is_none());
                        assert_eq!(
                            telemetry.trace_id().map(|id| id.as_str().to_owned()),
                            span.is_valid().then(|| span.trace_id().to_string())
                        );
                        roots.push(span);
                    });
                    scope.finish(chigiri::LifecycleResult::Succeeded);
                }
                let spans = records.take();
                if suppressed || decision == chigiri::SamplingDecision::Drop {
                    assert!(roots.iter().all(|root| !root.is_valid()));
                    assert_eq!(calls.load(Ordering::SeqCst), 0);
                    assert!(spans.is_empty());
                } else {
                    assert_ne!(roots[0].trace_id(), roots[1].trace_id());
                    assert_eq!(spans.len(), 2);
                    for span in spans {
                        assert_eq!(span.parent_span_id, opentelemetry::trace::SpanId::INVALID);
                        assert_eq!(span.links.links.len(), usize::from(has_parent));
                        if has_parent {
                            assert_eq!(span.links.links[0].span_context, parent(7));
                        }
                    }
                }
                assert_eq!(*Context::current().span().span_context(), parent(9));
                assert_eq!(Context::current().is_telemetry_suppressed(), !suppressed);
            }
        }
    }
}

fn suppression(context: Context, suppressed: bool) -> Context {
    if suppressed {
        context.with_telemetry_suppressed()
    } else {
        context
    }
}
