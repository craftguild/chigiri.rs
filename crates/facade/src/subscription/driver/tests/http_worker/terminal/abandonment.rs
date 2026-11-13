//! Abandonment at each actual preparation handoff.

mod abort;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::thread;
use std::time::Duration;

use chigiri_http::{ResponseCompletionOutcome, SubscriptionTransport};
use chigiri_runtime::LifecycleResult;

use super::application::{
    BlockingGate, CompletionBehavior, CompletionProbe, HookBehavior, SourceBehavior,
    install_completion, install_hook, install_source,
};
use super::support::{
    DeadlineFixture, VALID, WakeLatch, drain, endpoint, pending_with_deadlines, poll_once, run,
    runtime,
};
use crate::subscription::driver::tests::state::{
    EventMode, FactoryMode, HookMode, SetupMode, State,
};
use crate::transport::streaming::{InitialCommitTestBarrier, InitialCommitTestFault};

fn poll_until<F: Future>(
    runtime: &tokio::runtime::Runtime,
    future: &mut Pin<Box<F>>,
    wake: &Arc<WakeLatch>,
    ready: impl Fn() -> bool,
) {
    for _ in 0..200 {
        if ready() {
            return;
        }
        assert!(matches!(
            poll_once(runtime, future.as_mut(), wake),
            Poll::Pending
        ));
        thread::sleep(Duration::from_millis(10));
    }
    panic!("actual preparation did not reach the requested handoff");
}

fn queued_setup_drop(setup: SetupMode, expected_source_drops: usize) {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::modes(
        Vec::new(),
        setup,
        EventMode::Object,
        FactoryMode::Ready,
        FactoryMode::Ready,
        HookMode::Ready,
    );
    let (mut endpoint, signals) = endpoint(Arc::clone(&state));
    let callback_gate = BlockingGate::new();
    let hook_gate = BlockingGate::new();
    let completion = CompletionProbe::new(
        CompletionBehavior::Block(Arc::clone(&callback_gate)),
        None,
        false,
    );
    install_completion(&mut endpoint, Arc::clone(&state), Arc::clone(&completion));
    install_hook(
        &mut endpoint,
        Arc::clone(&state),
        HookBehavior::Block(Arc::clone(&hook_gate)),
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
            SubscriptionTransport::ServerSentEvents,
            VALID,
            fixture.deadlines,
        ),
    );
    let mut preparation = Box::pin(pending.prepare());
    let wake = WakeLatch::new();
    poll_until(&runtime, &mut preparation, &wake, || hook_gate.entered());
    let before = wake.count();
    hook_gate.release();
    wake.wait_after(before);
    let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
    let reserved = bytes.used();
    assert!(reserved > 0);
    drop(preparation);
    callback_gate.wait_entered();
    let completed = signals.completed();
    let retained_bytes = bytes.used();
    let outcomes = completion.outcomes();
    let source_drops = completion.source_drops_at_call();
    callback_gate.release();
    assert_eq!(completed, [(LifecycleResult::Cancelled, None)]);
    assert_eq!(retained_bytes, reserved);
    assert_eq!(outcomes, [ResponseCompletionOutcome::Failed]);
    assert_eq!(source_drops, [expected_source_drops]);
    drain(&runtime, guard);
    assert_eq!(bytes.used(), 0);
    assert_eq!(completion.calls(), 1);
}

#[test]
fn queued_ready_and_response_drop_without_wire_and_retain_cleanup_ownership() {
    queued_setup_drop(SetupMode::Ready, 1);
    queued_setup_drop(SetupMode::OuterError, 0);
}

#[test]
fn drop_while_response_hook_is_blocked_orders_source_before_failed_completion() {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::new(Vec::new());
    let (mut endpoint, signals) = endpoint(Arc::clone(&state));
    let hook_gate = BlockingGate::new();
    let completion = CompletionProbe::new(CompletionBehavior::Normal, None, false);
    install_completion(&mut endpoint, Arc::clone(&state), Arc::clone(&completion));
    install_hook(
        &mut endpoint,
        Arc::clone(&state),
        HookBehavior::Block(Arc::clone(&hook_gate)),
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
            SubscriptionTransport::ServerSentEvents,
            VALID,
            fixture.deadlines,
        ),
    );
    let mut preparation = Box::pin(pending.prepare());
    let wake = WakeLatch::new();
    poll_until(&runtime, &mut preparation, &wake, || hook_gate.entered());
    let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
    let reserved = bytes.used();
    drop(preparation);
    let completed = signals.completed();
    let retained_bytes = bytes.used();
    hook_gate.release();
    assert_eq!(completed, [(LifecycleResult::Cancelled, None)]);
    assert_eq!(retained_bytes, reserved);
    drain(&runtime, guard);
    assert_eq!(completion.calls(), 1);
    assert_eq!(completion.outcomes(), [ResponseCompletionOutcome::Failed]);
    assert_eq!(completion.source_drops_at_call(), [1]);
    assert_eq!(
        state
            .counts
            .source_drops
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(bytes.used(), 0);
}

#[test]
fn queued_callback_result_drop_keeps_encoded_once_without_failed_retry() {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::new(Vec::new());
    let (mut endpoint, signals) = endpoint(Arc::clone(&state));
    let callback_gate = BlockingGate::new();
    let source_gate = BlockingGate::new();
    install_source(
        &mut endpoint,
        Arc::clone(&state),
        SourceBehavior::BlockOnDrop(Arc::clone(&source_gate)),
    );
    signals.attach(&mut endpoint);
    let completion = CompletionProbe::new(
        CompletionBehavior::Block(Arc::clone(&callback_gate)),
        None,
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
            SubscriptionTransport::Multipart,
            VALID,
            fixture.deadlines,
        ),
    );
    let mut preparation = Box::pin(pending.prepare());
    let wake = WakeLatch::new();
    poll_until(&runtime, &mut preparation, &wake, || {
        callback_gate.entered()
    });
    let before = wake.count();
    callback_gate.release();
    wake.wait_after(before);
    let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
    let reserved = bytes.used();
    drop(preparation);
    source_gate.wait_entered();
    let completed = signals.completed();
    let retained_bytes = bytes.used();
    source_gate.release();
    assert_eq!(completed, [(LifecycleResult::Cancelled, None)]);
    assert_eq!(retained_bytes, reserved);
    drain(&runtime, guard);
    assert_eq!(bytes.used(), 0);
    assert_eq!(completion.calls(), 1);
    assert_eq!(completion.outcomes(), [ResponseCompletionOutcome::Encoded]);
    assert_eq!(completion.source_drops_at_call(), [0]);
    assert_eq!(
        state
            .counts
            .source_drops
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[test]
fn received_uncommitted_drop_is_cancelled_after_encoded_linearization() {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::new(Vec::new());
    let (mut endpoint, signals) = endpoint(Arc::clone(&state));
    let source_gate = BlockingGate::new();
    install_source(
        &mut endpoint,
        Arc::clone(&state),
        SourceBehavior::BlockOnDrop(Arc::clone(&source_gate)),
    );
    signals.attach(&mut endpoint);
    let completion = CompletionProbe::new(CompletionBehavior::Normal, None, false);
    install_completion(&mut endpoint, Arc::clone(&state), Arc::clone(&completion));
    let guard = endpoint
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let barrier = InitialCommitTestBarrier::new();
    let pending = run(
        &runtime,
        pending_with_deadlines(
            Arc::clone(&endpoint),
            SubscriptionTransport::ServerSentEvents,
            VALID,
            fixture.deadlines,
        ),
    );
    let mut preparation = Box::pin(
        pending
            .with_commit_fault(InitialCommitTestFault::PauseBeforeCommit(barrier.clone()))
            .prepare(),
    );
    let wake = WakeLatch::new();
    poll_until(&runtime, &mut preparation, &wake, || barrier.reached());
    let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
    let reserved = bytes.used();
    drop(preparation);
    barrier.release();
    source_gate.wait_entered();
    let completed = signals.completed();
    let retained_bytes = bytes.used();
    source_gate.release();
    assert_eq!(completed, [(LifecycleResult::Cancelled, None)]);
    assert_eq!(retained_bytes, reserved);
    drain(&runtime, guard);
    assert_eq!(completion.calls(), 1);
    assert_eq!(completion.outcomes(), [ResponseCompletionOutcome::Encoded]);
    assert_eq!(
        state
            .counts
            .source_drops
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(bytes.used(), 0);
}
