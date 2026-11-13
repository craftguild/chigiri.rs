//! Faults enter the actual admitted preparation after its deadline-first gate.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_http::ResponseCompletionOutcome;
use chigiri_runtime::LifecycleResult;

use super::super::state::{FactoryMode, State, count};
use super::support::{INVALID, VALID, drain, endpoint, pending, profiles, run, runtime};
use crate::subscription::control::MAX_INITIAL_RESPONSE_BYTES;
use crate::transport::streaming::{
    InitialCommitTestBarrier, InitialCommitTestFault, PreparedHttpSubscription,
};

#[derive(Clone, Copy, Debug)]
enum Path {
    Source,
    Validation,
    ContextRejection,
}

fn check_failure(fault: InitialCommitTestFault, maximum: bool) {
    let runtime = runtime();
    for profile in profiles() {
        for path in [Path::Source, Path::Validation, Path::ContextRejection] {
            if let InitialCommitTestFault::ShrinkAlways(applied) = &fault {
                applied.store(0, Ordering::SeqCst);
            }
            let mut probe = State::new(Vec::new());
            if matches!(path, Path::ContextRejection) {
                Arc::get_mut(&mut probe).unwrap().setup_factory = FactoryMode::Reject;
            }
            let (endpoint, signals) = endpoint(Arc::clone(&probe));
            let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
            let guard = endpoint
                .subscriptions
                .resources
                .register_worker_runtime()
                .unwrap();
            let body = if matches!(path, Path::Validation) {
                INVALID
            } else {
                VALID
            };
            let request = run(
                &runtime,
                pending(Arc::clone(&endpoint), profile, body, true),
            );
            let prepared = run(&runtime, request.with_commit_fault(fault.clone()).prepare());
            if let InitialCommitTestFault::ShrinkAlways(applied) = &fault {
                assert_eq!(
                    applied.load(Ordering::SeqCst),
                    1,
                    "persistent shrink fault must apply exactly once: {profile:?}/{path:?}"
                );
            }
            let PreparedHttpSubscription::Response(response) = prepared else {
                panic!("failed initial commit cannot publish a source or uncharged wire value");
            };
            assert_eq!(response.status(), 500);
            assert!(!response.is_streaming_profile());
            let body = std::str::from_utf8(response.as_ref()).unwrap();
            assert!(body.contains("Internal server error"));
            assert!(body.contains("INTERNAL_SERVER_ERROR"));
            assert!(!body.contains("original-secret"));
            assert!(!body.contains("Invariant"));
            assert_eq!(
                response.headers()[0].value,
                "application/json; charset=utf-8"
            );
            assert_eq!(
                signals.completed(),
                [(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"))]
            );
            assert_eq!(count(&probe.counts.source_polls), 0);
            let expected_completion = usize::from(matches!(path, Path::Source));
            assert_eq!(count(&probe.counts.completion_calls), expected_completion);
            if expected_completion == 1 {
                assert_eq!(
                    *probe.completion_outcomes.lock().unwrap(),
                    [ResponseCompletionOutcome::Encoded]
                );
            }
            let physical = Arc::new(response);
            let retained = Arc::clone(&physical);
            drop(physical);
            // Join must not release a wire owner's reservation, even on error.
            drain(&runtime, guard);
            assert_eq!(count(&probe.counts.source_drops), expected_completion);
            assert_eq!(
                endpoint
                    .in_flight
                    .load(std::sync::atomic::Ordering::Acquire),
                0
            );
            let expected_bytes = if maximum {
                MAX_INITIAL_RESPONSE_BYTES
            } else {
                retained.as_ref().as_ref().len()
                    + retained
                        .headers()
                        .iter()
                        .map(|header| 16 + header.name.len() + header.value.len())
                        .sum::<usize>()
            };
            assert_eq!(
                bytes.used(),
                expected_bytes,
                "wire reservation after source join: {profile:?}/{path:?}"
            );
            assert!(bytes.used() > 0);
            assert!(!bytes.is_corrupted());
            drop(retained);
            assert_eq!(bytes.used(), 0);
            assert_eq!(signals.completed().len(), 1);
        }
    }
}

#[test]
fn source_and_response_commit_failure_keep_exact_masked_wire_ownership() {
    check_failure(InitialCommitTestFault::Commit, false);
}

#[test]
fn exact_shrink_failure_recovers_without_re_admission_or_completion_retry() {
    check_failure(InitialCommitTestFault::ShrinkOnce, false);
}

#[test]
fn persistent_shrink_failure_keeps_the_same_maximum_through_last_wire_drop() {
    check_failure(
        InitialCommitTestFault::ShrinkAlways(Arc::new(AtomicUsize::new(0))),
        true,
    );
}

#[test]
fn pre_commit_worker_failure_is_exact_charged_without_applying_shrink_fault() {
    let runtime = runtime();
    for profile in profiles() {
        let mut probe = State::new(Vec::new());
        Arc::get_mut(&mut probe).unwrap().setup_factory = FactoryMode::Panic;
        let (endpoint, signals) = endpoint(Arc::clone(&probe));
        let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
        let guard = endpoint
            .subscriptions
            .resources
            .register_worker_runtime()
            .unwrap();
        let applied = Arc::new(AtomicUsize::new(0));
        let request = run(
            &runtime,
            pending(Arc::clone(&endpoint), profile, VALID, true),
        );
        let prepared = run(
            &runtime,
            request
                .with_commit_fault(InitialCommitTestFault::ShrinkAlways(Arc::clone(&applied)))
                .prepare(),
        );
        let PreparedHttpSubscription::Response(response) = prepared else {
            panic!("worker failure before commit must retain a finite masked response");
        };
        assert_eq!(applied.load(Ordering::SeqCst), 0, "{profile:?}");
        assert_eq!(response.status(), 500);
        assert!(!response.is_streaming_profile());
        assert_eq!(
            response.headers()[0].value,
            "application/json; charset=utf-8"
        );
        assert_eq!(response.as_ref().len(), 94);
        assert!(
            !std::str::from_utf8(response.as_ref())
                .unwrap()
                .contains("original-secret")
        );
        assert_eq!(
            bytes.used(),
            153,
            "pre-fault exact finite charge: {profile:?}"
        );
        assert_eq!(count(&probe.counts.factory_calls), 1);
        assert_eq!(count(&probe.counts.setup_calls), 0);
        assert_eq!(count(&probe.counts.completion_calls), 0);
        assert_eq!(
            signals.completed(),
            [(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"))]
        );
        drain(&runtime, guard);
        assert_eq!(bytes.used(), 153);
        assert!(!bytes.is_corrupted());
        drop(response);
        assert_eq!(bytes.used(), 0);
    }
}

#[test]
fn missing_physical_owner_closes_without_wire_or_source_publication() {
    check_closed(InitialCommitTestFault::MissingBuffer, false);
}

#[test]
fn corrupted_shared_owner_cannot_publish_an_already_reserved_response() {
    check_closed(InitialCommitTestFault::CorruptedBytes, true);
}

fn check_closed(fault: InitialCommitTestFault, corrupted: bool) {
    let runtime = runtime();
    for profile in profiles() {
        for body in [VALID, INVALID] {
            let probe = State::new(Vec::new());
            let (endpoint, signals) = endpoint(Arc::clone(&probe));
            let guard = endpoint
                .subscriptions
                .resources
                .register_worker_runtime()
                .unwrap();
            let request = run(
                &runtime,
                pending(Arc::clone(&endpoint), profile, body, false),
            );
            assert!(matches!(
                run(&runtime, request.with_commit_fault(fault.clone()).prepare()),
                PreparedHttpSubscription::Failed(_)
            ));
            assert_eq!(
                signals.completed(),
                [(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"))]
            );
            assert_eq!(count(&probe.counts.source_polls), 0);
            drain(&runtime, guard);
            assert_eq!(
                endpoint
                    .subscriptions
                    .resources
                    .delivery()
                    .byte_owner()
                    .used(),
                0
            );
            assert_eq!(
                endpoint
                    .subscriptions
                    .resources
                    .delivery()
                    .byte_owner()
                    .is_corrupted(),
                corrupted
            );
            assert_eq!(
                count(&probe.counts.completion_calls),
                usize::from(body == VALID)
            );
            assert_eq!(
                count(&probe.counts.source_drops),
                usize::from(body == VALID)
            );
        }
    }
}

#[test]
fn released_receipt_barrier_preserves_normal_committing() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(Vec::new());
        let (endpoint, signals) = endpoint(Arc::clone(&probe));
        let guard = endpoint
            .subscriptions
            .resources
            .register_worker_runtime()
            .unwrap();
        let barrier = InitialCommitTestBarrier::new();
        barrier.release();
        let request = run(
            &runtime,
            pending(Arc::clone(&endpoint), profile, VALID, false),
        );
        let prepared = run(
            &runtime,
            request
                .with_commit_fault(InitialCommitTestFault::PauseBeforeCommit(barrier.clone()))
                .prepare(),
        );
        assert!(barrier.reached());
        assert!(matches!(prepared, PreparedHttpSubscription::Ready(_)));
        assert_eq!(signals.completed(), [(LifecycleResult::Succeeded, None)]);
        drop(prepared);
        drain(&runtime, guard);
        assert_eq!(count(&probe.counts.completion_calls), 1);
        assert_eq!(
            endpoint
                .subscriptions
                .resources
                .delivery()
                .byte_owner()
                .used(),
            0
        );
    }
}
