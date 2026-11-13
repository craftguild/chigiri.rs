//! Original overall deadline identity and initial commit races.

use std::sync::Arc;

use chigiri_http::{ResponseCompletionOutcome, SubscriptionTransport};
use chigiri_runtime::LifecycleResult;

use super::application::{
    CompletionBehavior, CompletionProbe, HookBehavior, install_completion, install_hook,
};
use super::support::{
    DeadlineFixture, INVALID, VALID, drain, endpoint, pending_with_deadlines, run, runtime,
};
use crate::subscription::driver::tests::state::{State, count};
use crate::transport::streaming::PreparedHttpSubscription;

#[test]
fn phase_a_validation_keeps_one_original_overall_until_bounded_response_commit() {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::new(Vec::new());
    let (endpoint, signals) = endpoint(Arc::clone(&state));
    let guard = endpoint
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let pending = run(
        &runtime,
        pending_with_deadlines(
            Arc::clone(&endpoint),
            SubscriptionTransport::Multipart,
            INVALID,
            fixture.deadlines,
        ),
    );
    let PreparedHttpSubscription::Response(response) = run(&runtime, pending.prepare()) else {
        panic!("validation must return one bounded response");
    };
    assert_eq!(fixture.deadlines.overall.creations(), 1);
    assert!(fixture.deadlines.overall.polls() > 0);
    assert_eq!(fixture.deadlines.overall.drops(), 1);
    assert_eq!(count(&state.counts.factory_calls), 0);
    assert_eq!(signals.completed(), [(LifecycleResult::Succeeded, None)]);
    drop(response);
    drain(&runtime, guard);
}

#[test]
fn source_setup_keeps_the_original_overall_through_context_hook_and_completion() {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::new(Vec::new());
    let (mut endpoint, signals) = endpoint(Arc::clone(&state));
    let completion = CompletionProbe::new(
        CompletionBehavior::Normal,
        Some(&fixture.deadlines.overall),
        false,
    );
    install_completion(&mut endpoint, Arc::clone(&state), Arc::clone(&completion));
    let guard = endpoint
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let pending = run(
        &runtime,
        pending_with_deadlines(
            Arc::clone(&endpoint),
            SubscriptionTransport::ServerSentEvents,
            VALID,
            fixture.deadlines,
        ),
    );
    let PreparedHttpSubscription::Ready(ready) = run(&runtime, pending.prepare()) else {
        panic!("source setup must commit");
    };
    assert_eq!(fixture.deadlines.overall.creations(), 1);
    assert_eq!(completion.overall_drops_at_call(), [0]);
    assert_eq!(completion.outcomes(), [ResponseCompletionOutcome::Encoded]);
    assert_eq!(fixture.deadlines.overall.drops(), 1);
    assert_eq!(count(&state.counts.factory_calls), 1);
    assert_eq!(count(&state.counts.setup_calls), 1);
    assert_eq!(count(&state.counts.hook_calls), 1);
    assert_eq!(signals.completed(), [(LifecycleResult::Succeeded, None)]);
    drop(ready);
    drain(&runtime, guard);
}

#[test]
fn overall_ready_before_committing_wins_with_one_timed_out_completion() {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::new(Vec::new());
    let (mut endpoint, signals) = endpoint(Arc::clone(&state));
    let completion = CompletionProbe::new(CompletionBehavior::Normal, None, false);
    install_completion(&mut endpoint, Arc::clone(&state), Arc::clone(&completion));
    install_hook(
        &mut endpoint,
        Arc::clone(&state),
        HookBehavior::Trigger(&fixture.deadlines.overall),
    );
    let guard = endpoint
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let pending = run(
        &runtime,
        pending_with_deadlines(
            Arc::clone(&endpoint),
            SubscriptionTransport::Multipart,
            VALID,
            fixture.deadlines,
        ),
    );
    let PreparedHttpSubscription::Response(response) = run(&runtime, pending.prepare()) else {
        panic!("pre-commit deadline must return a finite response");
    };
    assert_eq!(response.status(), 504);
    assert_eq!(completion.calls(), 1);
    assert_eq!(completion.outcomes(), [ResponseCompletionOutcome::TimedOut]);
    assert_eq!(completion.source_drops_at_call(), [1]);
    assert_eq!(
        signals.completed(),
        [(LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"))]
    );
    drop(response);
    drain(&runtime, guard);
    assert_eq!(fixture.deadlines.overall.drops(), 1);
}

#[test]
fn committing_encoded_wins_even_when_callback_makes_overall_ready() {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::new(Vec::new());
    let (mut endpoint, signals) = endpoint(Arc::clone(&state));
    let completion = CompletionProbe::new(
        CompletionBehavior::Normal,
        Some(&fixture.deadlines.overall),
        true,
    );
    install_completion(&mut endpoint, Arc::clone(&state), Arc::clone(&completion));
    let guard = endpoint
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let pending = run(
        &runtime,
        pending_with_deadlines(
            Arc::clone(&endpoint),
            SubscriptionTransport::ServerSentEvents,
            VALID,
            fixture.deadlines,
        ),
    );
    let PreparedHttpSubscription::Ready(ready) = run(&runtime, pending.prepare()) else {
        panic!("Committing must not be replaced by a late timeout");
    };
    assert_eq!(completion.calls(), 1);
    assert_eq!(completion.outcomes(), [ResponseCompletionOutcome::Encoded]);
    assert_eq!(completion.overall_drops_at_call(), [0]);
    assert_eq!(signals.completed(), [(LifecycleResult::Succeeded, None)]);
    drop(ready);
    drain(&runtime, guard);
    assert_eq!(fixture.deadlines.overall.drops(), 1);
}
