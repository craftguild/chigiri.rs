//! Error conversion obeys control active inside the owning execution.

use std::sync::atomic::Ordering;

use chigiri_runtime::instrumentation::{Instrumentation, InstrumentationKind};
use chigiri_runtime::{
    ErrorEvent, LifecyclePhase, LifecycleResult, RequestIdentity, SamplingDecision,
};
use opentelemetry::Context;

use super::support::provider;

#[test]
fn error_conversion_keeps_nested_owning_suppression() {
    let (_owner, backend, records) = provider();
    let root = backend.start(
        &backend.capture(&()),
        InstrumentationKind::Request,
        SamplingDecision::Record,
    );
    root.run(|| {
        let _suppressed = Context::current().with_telemetry_suppressed().attach();
        let source = std::io::Error::other("private error source");
        let event = ErrorEvent::new(
            None,
            RequestIdentity::new(None, None, None),
            LifecyclePhase::Execution,
            "INTERNAL_SERVER_ERROR",
            &source,
            None,
            None,
            "resolver",
        );
        root.observe_error(&event);
        assert!(Context::current().is_telemetry_suppressed());
    });
    root.finish(LifecycleResult::Succeeded);
    assert_eq!(backend.tracer().calls.load(Ordering::SeqCst), 1);
    let ended = records.take();
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].name, "graphql.request");
    assert!(!Context::current().is_telemetry_suppressed());
}
