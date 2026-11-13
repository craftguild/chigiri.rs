//! Unavailable recording never erases an already selected propagation context.

use std::sync::atomic::Ordering;

use chigiri_runtime::SamplingDecision;
use chigiri_runtime::instrumentation::{Instrumentation, InstrumentationKind};
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, TraceContextExt};

use super::support::{parent, provider};

#[test]
fn unavailable_provider_preserves_parent_control_and_fresh_event_boundary() {
    for suppressed in [false, true] {
        for event in [false, true] {
            let (owner, backend, records) = provider();
            let values = {
                let context = parent(8, true);
                let context = if suppressed {
                    context.with_telemetry_suppressed()
                } else {
                    context
                };
                let _guard = context.attach();
                backend.capture(&())
            };
            owner.shutdown().unwrap();
            let worker = parent(9, true);
            let worker = if suppressed {
                worker
            } else {
                worker.with_telemetry_suppressed()
            };
            let _worker = worker.attach();
            let kind = if event {
                InstrumentationKind::SubscriptionEvent
            } else {
                InstrumentationKind::Request
            };
            let scope = backend.start(&values, kind, SamplingDecision::Record);
            scope.run(|| {
                let current = Context::current();
                let expected = if event {
                    SpanContext::NONE
                } else {
                    parent(8, true).span().span_context().clone()
                };
                assert_eq!(*current.span().span_context(), expected);
                assert_eq!(current.is_telemetry_suppressed(), suppressed);
                let child = scope.child(InstrumentationKind::Execution, SamplingDecision::Record);
                child.run(|| {
                    assert_eq!(*Context::current().span().span_context(), expected);
                    assert_eq!(Context::current().is_telemetry_suppressed(), suppressed);
                });
            });
            assert_eq!(
                backend.tracer().calls.load(Ordering::SeqCst),
                usize::from(!suppressed)
            );
            assert_eq!(backend.shared.active.load(Ordering::SeqCst), 0);
            assert!(records.take().is_empty());
            assert_eq!(
                *Context::current().span().span_context(),
                *parent(9, true).span().span_context()
            );
            assert_eq!(Context::current().is_telemetry_suppressed(), !suppressed);
        }
    }
}
