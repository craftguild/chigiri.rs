//! Public observer regressions using actual released OpenTelemetry SDK spans.

#[path = "legacy_suppression/support.rs"]
mod support;

use chigiri_runtime::{ErrorEvent, ErrorObserver, LifecyclePhase, LifecycleResult};
use opentelemetry::Context;

use support::{emit, fixture, telemetry};

#[test]
fn suppressed_completion_ends_and_drops_every_owned_span() {
    for result in [
        LifecycleResult::Succeeded,
        LifecycleResult::Rejected,
        LifecycleResult::Failed,
        LifecycleResult::TimedOut,
        LifecycleResult::Cancelled,
    ] {
        let (_provider, observer, counts) = fixture();
        let request = telemetry(Some("owned-hierarchy"));
        for phase in [
            LifecyclePhase::RequestStarted,
            LifecyclePhase::OperationSourceResolution,
            LifecyclePhase::Execution,
        ] {
            emit(&observer, &request, phase, LifecycleResult::Started, "test");
        }
        counts.assert(4, 0, 0);

        let guard = Context::new().with_telemetry_suppressed().attach();
        emit(
            &observer,
            &request,
            LifecyclePhase::RequestCompleted,
            result,
            "test",
        );
        // The observer stays alive; completion must end and physically release
        // the request, operation, provider and execution spans exactly once.
        counts.assert(4, 4, 4);
        emit(
            &observer,
            &request,
            LifecyclePhase::RequestCompleted,
            result,
            "test",
        );
        assert!(Context::current().is_telemetry_suppressed());
        counts.assert(4, 4, 4);
        drop(guard);
        assert!(!Context::current().is_telemetry_suppressed());
        drop(observer);
        counts.assert(4, 4, 4);
    }
}

#[test]
fn suppressed_phases_close_existing_spans_without_creating_replacements() {
    let (_provider, observer, counts) = fixture();
    let request = telemetry(Some("phase-cleanup"));
    for phase in [
        LifecyclePhase::RequestStarted,
        LifecyclePhase::OperationSourceResolution,
    ] {
        emit(&observer, &request, phase, LifecycleResult::Started, "test");
    }
    counts.assert(3, 0, 0);

    {
        let _guard = Context::new().with_telemetry_suppressed().attach();
        emit(
            &observer,
            &request,
            LifecyclePhase::OperationSourceResolution,
            LifecycleResult::Succeeded,
            "test",
        );
        counts.assert(3, 1, 1);
        for phase in [
            LifecyclePhase::Execution,
            LifecyclePhase::TransportAdmission,
        ] {
            emit(&observer, &request, phase, LifecycleResult::Started, "test");
        }
        emit(
            &observer,
            &request,
            LifecyclePhase::Execution,
            LifecycleResult::Succeeded,
            "field",
        );
        observer.observe_error(ErrorEvent::new(
            None,
            request.identity(None),
            LifecyclePhase::Execution,
            "INTERNAL_SERVER_ERROR",
            &std::io::Error::other("fixture error"),
            None,
            None,
            "test",
        ));
        // No request entry and no ID must both avoid short fallback spans.
        for absent in [telemetry(Some("absent")), telemetry(None)] {
            for (phase, result) in [
                (LifecyclePhase::RequestStarted, LifecycleResult::Started),
                (LifecyclePhase::Execution, LifecycleResult::Succeeded),
                (LifecyclePhase::RequestCompleted, LifecycleResult::Succeeded),
            ] {
                emit(&observer, &absent, phase, result, "test");
            }
        }
        counts.assert(3, 1, 1);
    }

    emit(
        &observer,
        &request,
        LifecyclePhase::Execution,
        LifecycleResult::Started,
        "test",
    );
    counts.assert(4, 1, 1);
    {
        let _guard = Context::new().with_telemetry_suppressed().attach();
        emit(
            &observer,
            &request,
            LifecyclePhase::Execution,
            LifecycleResult::Succeeded,
            "field",
        );
        counts.assert(4, 1, 1);
        for _ in 0..2 {
            emit(
                &observer,
                &request,
                LifecyclePhase::Execution,
                LifecycleResult::Cancelled,
                "test",
            );
            counts.assert(4, 2, 2);
        }
        emit(
            &observer,
            &request,
            LifecyclePhase::RequestCompleted,
            LifecycleResult::Cancelled,
            "test",
        );
        counts.assert(4, 4, 4);
    }
    drop(observer);
    counts.assert(4, 4, 4);
}

#[test]
fn suppressed_completions_release_retained_capacity_for_later_roots() {
    let (_provider, observer, counts) = fixture();
    // Exercise the existing 4,096-entry legacy retention limit using only the
    // public API, with constant-memory SDK counters rather than an exporter.
    for index in 0..4096 {
        let request = telemetry(Some(&format!("completed-{index}")));
        emit(
            &observer,
            &request,
            LifecyclePhase::RequestStarted,
            LifecycleResult::Started,
            "test",
        );
        let _guard = Context::new().with_telemetry_suppressed().attach();
        emit(
            &observer,
            &request,
            LifecyclePhase::RequestCompleted,
            LifecycleResult::Succeeded,
            "test",
        );
    }
    counts.assert(4096, 4096, 4096);
    let next = telemetry(Some("next-root"));
    emit(
        &observer,
        &next,
        LifecyclePhase::RequestStarted,
        LifecycleResult::Started,
        "test",
    );
    counts.assert(4097, 4096, 4096);
    emit(
        &observer,
        &next,
        LifecyclePhase::RequestCompleted,
        LifecycleResult::Succeeded,
        "test",
    );
    counts.assert(4097, 4097, 4097);
    drop(observer);
    counts.assert(4097, 4097, 4097);
}
