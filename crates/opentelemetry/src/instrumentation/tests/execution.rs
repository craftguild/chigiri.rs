use std::sync::Arc;
use std::sync::atomic::Ordering;

use chigiri_runtime::instrumentation::{Instrumentation, InstrumentationKind};
use chigiri_runtime::{LifecycleResult, SamplingDecision};
use opentelemetry::Context;
use opentelemetry::trace::{SpanId, TraceContextExt};

use super::support::{parent, provider};

#[test]
fn real_sdk_parentage_sampling_suppression_and_restoration_are_independent() {
    for suppressed in [false, true] {
        for valid_parent in [false, true] {
            for sampled in [false, true] {
                for decision in [SamplingDecision::Record, SamplingDecision::Drop] {
                    let (_owner, backend, records) = provider();
                    let base = if valid_parent {
                        parent(8, sampled)
                    } else {
                        Context::new()
                    };
                    let base = if suppressed {
                        base.with_telemetry_suppressed()
                    } else {
                        base
                    };
                    let values = {
                        let _guard = base.attach();
                        backend.capture(&())
                    };
                    let scope = backend.start(&values, InstrumentationKind::Request, decision);
                    let telemetry = chigiri_runtime::RequestTelemetry::new(
                        None,
                        Some(chigiri_runtime::TraceId::new("opaque-correlation").unwrap()),
                        decision,
                    )
                    .with_instrumentation_scope(scope.clone());
                    let worker = parent(9, true).with_telemetry_suppressed();
                    let _worker = worker.attach();
                    scope.run(|| {
                        let current = Context::current();
                        assert_eq!(current.is_telemetry_suppressed(), suppressed);
                        let span = current.span();
                        let selected = span.span_context();
                        let created = decision == SamplingDecision::Record && !suppressed;
                        assert_eq!(selected.is_valid(), valid_parent || created);
                        assert_eq!(
                            telemetry.trace_id().unwrap().as_str(),
                            if selected.is_valid() {
                                selected.trace_id().to_string()
                            } else {
                                "opaque-correlation".to_owned()
                            }
                        );
                        if valid_parent {
                            assert_eq!(selected.trace_id().to_bytes(), [8; 16]);
                            assert_eq!(selected.is_sampled(), sampled);
                        }
                        assert_eq!(
                            backend.tracer().calls.load(Ordering::SeqCst),
                            usize::from(created)
                        );
                    });
                    assert_eq!(
                        Context::current()
                            .span()
                            .span_context()
                            .trace_id()
                            .to_bytes(),
                        [9; 16]
                    );
                    assert!(Context::current().is_telemetry_suppressed());
                    scope.finish(LifecycleResult::Succeeded);
                    scope.finish(LifecycleResult::Failed);
                    drop(scope);
                    let recorded = decision == SamplingDecision::Record
                        && !suppressed
                        && (!valid_parent || sampled);
                    let ended = records.take();
                    assert_eq!(ended.len(), usize::from(recorded));
                    if recorded {
                        assert_eq!(
                            ended[0].parent_span_id,
                            if valid_parent {
                                SpanId::from_bytes([8; 8])
                            } else {
                                SpanId::INVALID
                            }
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn fresh_events_link_at_creation_and_never_adopt_workers_or_previous_events() {
    let (_owner, backend, records) = provider();
    let setup_values = {
        let _guard = parent(1, true).attach();
        backend.capture(&())
    };
    let setup = backend.start(
        &setup_values,
        InstrumentationKind::SubscriptionSetup,
        SamplingDecision::Record,
    );
    let retained = setup.context();
    setup.finish(LifecycleResult::Succeeded);
    drop(setup);
    let setup_record = records.take().pop().unwrap();
    assert_eq!(setup_record.name, "graphql.subscription.setup");
    let _worker = parent(2, true).with_telemetry_suppressed().attach();
    let mut traces = Vec::new();
    for _ in 0..3 {
        let event = backend.start(
            &retained,
            InstrumentationKind::SubscriptionEvent,
            SamplingDecision::Record,
        );
        event.run(|| {
            let current = Context::current();
            assert!(!current.is_telemetry_suppressed());
            traces.push(current.span().span_context().trace_id());
        });
        event.finish(LifecycleResult::Succeeded);
    }
    let ended = records.take();
    assert_eq!(ended.len(), 3);
    for (index, event) in ended.iter().enumerate() {
        assert_eq!(event.parent_span_id, SpanId::INVALID);
        assert_eq!(event.links.links.len(), 1);
        assert_eq!(event.links.links[0].span_context, setup_record.span_context);
        assert_ne!(
            event.span_context.trace_id(),
            setup_record.span_context.trace_id()
        );
        assert_eq!(event.span_context.trace_id(), traces[index]);
        assert!(!traces[..index].contains(&traces[index]));
    }
}

#[test]
fn nested_suppression_and_capacity_decline_do_not_reactivate_sdk_spans() {
    let (_owner, mut backend, records) = provider();
    Arc::get_mut(&mut backend.shared).unwrap().limit = 1;
    let root = backend.start(
        &backend.capture(&()),
        InstrumentationKind::Request,
        SamplingDecision::Record,
    );
    root.run(|| {
        let _nested = Context::current().with_telemetry_suppressed().attach();
        let child = root.child(InstrumentationKind::Execution, SamplingDecision::Record);
        child.run(|| assert!(Context::current().is_telemetry_suppressed()));
    });
    let suppressed = chigiri_runtime::instrumentation::PropagationContext::new(None, true);
    let dropped = suppressed.retain_within(0);
    let event = backend.start(
        &dropped,
        InstrumentationKind::SubscriptionEvent,
        SamplingDecision::Record,
    );
    event.run(|| {
        assert!(Context::current().is_telemetry_suppressed());
        assert!(!Context::current().span().span_context().is_valid());
    });
    let overflow = backend.start(
        &root.context(),
        InstrumentationKind::Request,
        SamplingDecision::Record,
    );
    overflow.run(|| {
        assert_eq!(
            Context::current()
                .span()
                .span_context()
                .trace_id()
                .to_bytes(),
            root.context().parent().unwrap().trace_id()
        )
    });
    assert_eq!(backend.tracer().calls.load(Ordering::SeqCst), 1);
    root.finish(LifecycleResult::Succeeded);
    assert_eq!(backend.shared.active.load(Ordering::SeqCst), 1);
    drop(root);
    assert_eq!(backend.shared.active.load(Ordering::SeqCst), 0);
    assert_eq!(records.take().len(), 1);
}
