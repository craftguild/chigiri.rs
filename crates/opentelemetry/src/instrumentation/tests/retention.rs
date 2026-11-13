//! Idle snapshots retain values; only an actually owned span needs a Context cache.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chigiri_runtime::instrumentation::{
    Instrumentation, InstrumentationKind, PropagationContext, TraceParent,
};
use chigiri_runtime::{LifecycleResult, SamplingDecision};
use opentelemetry::Context;
use opentelemetry::trace::TraceContextExt;

use super::support::{parent, provider};

#[derive(Clone)]
struct Private;

#[test]
fn lazy_activation_keeps_released_state_validation_without_a_retained_parser_allocation() {
    for suppressed in [false, true] {
        for (state, expected) in [("vendor=value", "vendor=value"), ("bad key=value", "")] {
            let (_owner, backend, records) = provider();
            let values = PropagationContext::new(
                Some(TraceParent::new([7; 16], [7; 8], 1, true, state).unwrap()),
                suppressed,
            );
            let scope = backend.start(
                &values,
                InstrumentationKind::SubscriptionSetup,
                SamplingDecision::Drop,
            );
            assert_eq!(backend.shared.retained_contexts.load(Ordering::SeqCst), 0);
            assert_eq!(scope.context().parent().unwrap().trace_state(), expected);
            scope.run(|| {
                assert_eq!(
                    Context::current()
                        .span()
                        .span_context()
                        .trace_state()
                        .header(),
                    expected
                );
                assert_eq!(Context::current().is_telemetry_suppressed(), suppressed);
            });
            assert_eq!(backend.shared.retained_contexts.load(Ordering::SeqCst), 0);
            assert_eq!(backend.tracer().calls.load(Ordering::SeqCst), 0);
            assert!(records.take().is_empty());
        }
    }
}

#[test]
fn idle_unrecorded_scopes_keep_no_cached_context_beyond_budgeted_values() {
    let state = (0..32)
        .map(|i| format!("k{i}=value{i:02}"))
        .collect::<Vec<_>>()
        .join(",");
    assert!(state.len() <= 512);
    for suppressed in [false, true] {
        for has_parent in [true, false] {
            for sampled in [false, true] {
                for kind in [
                    InstrumentationKind::Request,
                    InstrumentationKind::SubscriptionSetup,
                    InstrumentationKind::SubscriptionEvent,
                ] {
                    for sampling in [SamplingDecision::Record, SamplingDecision::Drop] {
                        for capacity in [0, 1] {
                            let (_owner, mut backend, records) = provider();
                            Arc::get_mut(&mut backend.shared).unwrap().limit = capacity;
                            let values = PropagationContext::new(
                                has_parent.then(|| {
                                    TraceParent::new(
                                        [7; 16],
                                        [7; 8],
                                        u8::from(sampled),
                                        true,
                                        &state,
                                    )
                                    .unwrap()
                                }),
                                suppressed,
                            );
                            let scope = backend.start(&values, kind, sampling);
                            let owned = !suppressed
                                && sampling == SamplingDecision::Record
                                && capacity != 0;
                            assert_eq!(
                                backend.shared.retained_contexts.load(Ordering::SeqCst),
                                usize::from(owned),
                                "idle {kind:?}/{suppressed}/{has_parent}/{sampled}/{sampling:?}/{capacity}"
                            );
                            let worker = parent(9, true).with_value(Private);
                            let worker = if suppressed {
                                worker
                            } else {
                                worker.with_telemetry_suppressed()
                            };
                            let _guard = worker.attach();
                            for _ in 0..3 {
                                scope.run(|| {
                                    let current = Context::current();
                                    assert_eq!(current.is_telemetry_suppressed(), suppressed);
                                    assert!(current.get::<Private>().is_none());
                                    let span = current.span().span_context().clone();
                                    if !owned {
                                        let retained =
                                            if kind == InstrumentationKind::SubscriptionEvent {
                                                values.without_parent()
                                            } else {
                                                values.clone()
                                            };
                                        assert_eq!(scope.context(), retained);
                                        assert_eq!(span.is_valid(), retained.parent().is_some());
                                        if has_parent
                                            && kind != InstrumentationKind::SubscriptionEvent
                                        {
                                            assert_eq!(span.trace_id().to_bytes(), [7; 16]);
                                            assert_eq!(span.span_id().to_bytes(), [7; 8]);
                                            assert_eq!(span.trace_state().header(), state);
                                            assert_eq!(span.is_sampled(), sampled);
                                        }
                                    }
                                });
                                assert_eq!(
                                    Context::current()
                                        .span()
                                        .span_context()
                                        .trace_id()
                                        .to_bytes(),
                                    [9; 16]
                                );
                                assert_eq!(
                                    Context::current().is_telemetry_suppressed(),
                                    !suppressed
                                );
                                assert!(Context::current().get::<Private>().is_some());
                                assert_eq!(
                                    backend.shared.retained_contexts.load(Ordering::SeqCst),
                                    usize::from(owned)
                                );
                            }
                            scope.finish(LifecycleResult::Succeeded);
                            assert_eq!(
                                backend.shared.active.load(Ordering::SeqCst),
                                usize::from(owned)
                            );
                            drop(scope);
                            assert_eq!(backend.shared.retained_contexts.load(Ordering::SeqCst), 0);
                            assert_eq!(backend.shared.active.load(Ordering::SeqCst), 0);
                            let recording = owned
                                && (kind == InstrumentationKind::SubscriptionEvent
                                    || !has_parent
                                    || sampled);
                            assert_eq!(records.take().len(), usize::from(recording));
                        }
                    }
                }
            }
        }
    }
}
