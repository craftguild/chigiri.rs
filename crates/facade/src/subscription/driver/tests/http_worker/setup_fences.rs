//! ADR-0055 task-side HTTP setup cancellation, without an outside owner poll.

mod completion;
mod support;

use std::sync::Arc;

use chigiri_http::ResponseCompletionOutcome;
use chigiri_runtime::LifecyclePhase;

use super::super::state::{EventMode, FactoryMode, HookMode, SetupMode, SourceStep, State, count};
use super::support::{VALID, profiles, run};
use crate::transport::streaming::PreparedHttpSubscription;
use crate::{ShutdownOptions, ShutdownOutcome};
use support::{Fixture, reached};

fn stop(fixture: &Fixture, mode: usize) {
    let now = tokio::time::Instant::now();
    match mode {
        0 => {
            fixture.control.begin(ShutdownOptions::immediate(), now);
        }
        1 => {
            fixture.control.begin(ShutdownOptions::graceful(), now);
        }
        2 => {
            fixture.control.begin(
                ShutdownOptions::graceful(),
                now - std::time::Duration::from_secs(31),
            );
            fixture.control.expire(now);
            assert_eq!(fixture.control.outcome(), Some(ShutdownOutcome::Expired));
        }
        _ => unreachable!(),
    }
}

fn cancelled(
    fixture: &Fixture,
    preparation: impl std::future::Future<Output = PreparedHttpSubscription>,
) {
    let PreparedHttpSubscription::Response(response) = run(&fixture.runtime, preparation) else {
        panic!("cancelled preparation must be a safe finite response");
    };
    assert_eq!(response.status(), 503);
    assert!(response.as_ref().is_empty());
    assert!(response.headers().is_empty());
    assert!(!response.is_streaming_profile());
    assert_eq!(count(&fixture.boundary.errors), 0);
    assert_eq!(fixture.cancelled_requests(), 1);
}

#[test]
fn stop_before_validation_prevents_scalar_context_and_source_work() {
    const COERCION: &[u8] = br#"{"query":"subscription Named($value: Probe!) { events(limit: 2, probe: $value) { value } }","operationName":"Named","variables":{"value":"worker-value"}}"#;
    for profile in profiles() {
        for mode in 0..3 {
            let probe = State::new(Vec::new());
            let fixture = Fixture::new(probe.clone(), None);
            let preparation = fixture.preparation(profile, COERCION);
            stop(&fixture, mode);
            cancelled(&fixture, preparation);
            assert_eq!(count(&probe.counts.scalar_calls), 0);
            assert_eq!(count(&probe.counts.factory_calls), 0);
            assert_eq!(count(&probe.counts.setup_calls), 0);
            assert_eq!(count(&probe.counts.completion_calls), 0);
            fixture.finish();
        }
    }
}

#[test]
fn accepted_stop_at_context_source_or_hook_boundary_blocks_the_following_phase() {
    for profile in profiles() {
        for phase in [
            LifecyclePhase::ContextConstruction,
            LifecyclePhase::Execution,
            LifecyclePhase::ResponseHook,
        ] {
            for mode in 0..3 {
                let probe = State::new(vec![SourceStep::Pending]);
                let mut fixture = Fixture::new(probe.clone(), Some(phase));
                let mut preparation = Box::pin(fixture.preparation(profile, VALID));
                fixture.reach(preparation.as_mut(), || reached(&fixture.boundary));
                stop(&fixture, mode);
                fixture.release();
                // No outer preparation poll can drive or cancel the child now.
                fixture.finished();
                // Native task completion alone is not a consumed join. The
                // original outside owner still holds its physical admission.
                assert_eq!(
                    fixture
                        .endpoint
                        .subscriptions
                        .resources
                        .delivery()
                        .snapshot()
                        .endpoint_active_subscriptions,
                    1
                );
                assert_eq!(count(&probe.counts.factory_calls), 1);
                assert_eq!(
                    count(&probe.counts.setup_calls),
                    usize::from(phase != LifecyclePhase::ContextConstruction)
                );
                assert_eq!(
                    count(&probe.counts.hook_calls),
                    usize::from(phase == LifecyclePhase::ResponseHook)
                );
                assert_eq!(count(&probe.counts.source_polls), 0);
                assert_eq!(count(&probe.counts.completion_calls), 1);
                assert_eq!(count(&probe.counts.completion_drops), 1);
                let outcome = if mode == 2 {
                    ResponseCompletionOutcome::TimedOut
                } else {
                    ResponseCompletionOutcome::Failed
                };
                assert_eq!(*probe.completion_outcomes.lock().unwrap(), [outcome]);
                let before = probe.completion_drop_counts.lock().unwrap().clone();
                assert_eq!(
                    before[0][0],
                    usize::from(phase != LifecyclePhase::ContextConstruction)
                );
                assert_eq!(before[0][3], 1, "context must be dropped before completion");
                cancelled(&fixture, preparation);
                fixture.finish();
            }
        }
    }
}

#[test]
fn pending_context_source_and_hook_cancel_inside_the_same_task_without_parent_polling() {
    for profile in profiles() {
        for stage in 0..3 {
            for mode in 0..3 {
                let probe = State::modes(
                    Vec::new(),
                    if stage == 1 {
                        SetupMode::Pending
                    } else {
                        SetupMode::Ready
                    },
                    EventMode::Object,
                    if stage == 0 {
                        FactoryMode::Pending
                    } else {
                        FactoryMode::Ready
                    },
                    FactoryMode::Ready,
                    if stage == 2 {
                        HookMode::Pending
                    } else {
                        HookMode::Ready
                    },
                );
                let fixture = Fixture::new(probe.clone(), None);
                let mut preparation = Box::pin(fixture.preparation(profile, VALID));
                let counter = match stage {
                    0 => &probe.counts.factory_calls,
                    1 => &probe.counts.setup_calls,
                    2 => &probe.counts.hook_calls,
                    _ => unreachable!(),
                };
                fixture.reach(preparation.as_mut(), || count(counter) == 1);
                stop(&fixture, mode);
                fixture.finished();
                assert_eq!(
                    fixture
                        .endpoint
                        .subscriptions
                        .resources
                        .delivery()
                        .snapshot()
                        .endpoint_active_subscriptions,
                    1
                );
                let drops = match stage {
                    0 => &probe.counts.factory_future_drops,
                    1 => &probe.counts.setup_future_drops,
                    2 => &probe.counts.hook_future_drops,
                    _ => unreachable!(),
                };
                assert_eq!(count(drops), 1);
                assert_eq!(count(&probe.counts.source_polls), 0);
                assert_eq!(
                    count(&probe.counts.completion_calls),
                    usize::from(stage != 0)
                );
                assert_eq!(
                    count(&probe.counts.completion_drops),
                    usize::from(stage != 0)
                );
                if stage != 0 {
                    let outcome = if mode == 2 {
                        ResponseCompletionOutcome::TimedOut
                    } else {
                        ResponseCompletionOutcome::Failed
                    };
                    assert_eq!(*probe.completion_outcomes.lock().unwrap(), [outcome]);
                    let before = probe.completion_drop_counts.lock().unwrap().clone();
                    assert_eq!(before[0][3], 1);
                    if stage == 1 {
                        assert_eq!(before[0][1], 1);
                    }
                    if stage == 2 {
                        assert_eq!(before[0][2], 1);
                        assert_eq!(before[0][0], 1);
                    }
                }
                cancelled(&fixture, preparation);
                fixture.finish();
            }
        }
    }
}

#[test]
fn infrastructure_stop_is_observed_as_failure_instead_of_successful_cancellation() {
    for profile in profiles() {
        let probe = State::new(Vec::new());
        let fixture = Fixture::new(Arc::clone(&probe), None);
        let preparation = fixture.preparation(profile, VALID);
        fixture.control.fail();
        let PreparedHttpSubscription::Response(response) = run(&fixture.runtime, preparation)
        else {
            panic!("bounded masked failure");
        };
        assert_eq!(response.status(), 500);
        assert_eq!(count(&fixture.boundary.errors), 1);
        assert_eq!(fixture.cancelled_requests(), 0);
        assert_eq!(count(&probe.counts.factory_calls), 0);
        assert_eq!(
            fixture.control.outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        drop(response);
        fixture.finish();
    }
}
