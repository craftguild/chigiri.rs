//! Callback and destructor failure containment.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;
use std::thread;
use std::time::{Duration, Instant};

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
use crate::subscription::driver::tests::state::State;
use crate::transport::streaming::PreparedHttpSubscription;

fn callback_failure(behavior: CompletionBehavior) {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::new(Vec::new());
    let (mut endpoint, signals) = endpoint(Arc::clone(&state));
    let completion = CompletionProbe::new(behavior, None, false);
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
    let PreparedHttpSubscription::Response(response) = run(&runtime, pending.prepare()) else {
        panic!("callback failure must be a bounded masked response");
    };
    assert_eq!(response.status(), 500);
    assert!(
        std::str::from_utf8(response.as_ref())
            .unwrap()
            .contains("INTERNAL_SERVER_ERROR")
    );
    assert_eq!(completion.calls(), 1);
    assert_eq!(completion.outcomes(), [ResponseCompletionOutcome::Encoded]);
    assert_eq!(
        signals.completed(),
        [(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"))]
    );
    let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
    assert!(bytes.used() > 0);
    drain(&runtime, guard);
    assert!(bytes.used() > 0);
    assert_eq!(completion.source_drops_at_call(), [0]);
    assert_eq!(state.counts.source_drops.load(Ordering::SeqCst), 1);
    drop(response);
    assert_eq!(bytes.used(), 0);
}

#[test]
fn callback_panic_and_oversized_return_are_masked_once_without_retry() {
    callback_failure(CompletionBehavior::Panic);
    callback_failure(CompletionBehavior::Oversize);
}

#[test]
fn source_and_nested_callback_destructor_panics_are_contained_before_masked_failure() {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::new(Vec::new());
    let payload_drops = Arc::new(AtomicUsize::new(0));
    let (mut endpoint, signals) = endpoint(Arc::clone(&state));
    install_source(
        &mut endpoint,
        Arc::clone(&state),
        SourceBehavior::PanicOnDrop,
    );
    signals.attach(&mut endpoint);
    let completion = CompletionProbe::new(
        CompletionBehavior::NestedPanic(Arc::clone(&payload_drops)),
        None,
        false,
    );
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
            SubscriptionTransport::ServerSentEvents,
            VALID,
            fixture.deadlines,
        ),
    );
    let PreparedHttpSubscription::Response(response) = run(&runtime, pending.prepare()) else {
        panic!("nested disposal failure must remain a bounded response");
    };
    assert_eq!(response.status(), 500);
    assert_eq!(completion.calls(), 1);
    assert_eq!(completion.outcomes(), [ResponseCompletionOutcome::TimedOut]);
    assert_eq!(completion.source_drops_at_call(), [1]);
    assert_eq!(payload_drops.load(Ordering::SeqCst), 1);
    assert_eq!(
        signals.completed(),
        [(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"))]
    );
    drop(response);
    drain(&runtime, guard);
    assert_eq!(completion.calls(), 1);
    assert_eq!(state.counts.source_drops.load(Ordering::SeqCst), 1);
}

#[test]
fn blocked_source_drop_crosses_real_cleanup_wait_and_releases_after_physical_join() {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::new(Vec::new());
    let drop_gate = BlockingGate::new();
    let (mut endpoint, signals) = endpoint(Arc::clone(&state));
    install_source(
        &mut endpoint,
        Arc::clone(&state),
        SourceBehavior::BlockOnDrop(Arc::clone(&drop_gate)),
    );
    signals.attach(&mut endpoint);
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
    let mut preparation = Box::pin(pending.prepare());
    let wake = WakeLatch::new();
    let started = Instant::now();
    for _ in 0..200 {
        if drop_gate.entered() {
            break;
        }
        assert!(matches!(
            poll_once(&runtime, preparation.as_mut(), &wake),
            Poll::Pending
        ));
        thread::sleep(Duration::from_millis(10));
    }
    assert!(drop_gate.entered());
    let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
    assert!(bytes.used() > 0);
    let prepared = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(7), preparation.as_mut())
            .await
            .expect("real five-second cleanup bound")
    });
    let PreparedHttpSubscription::Response(response) = prepared else {
        panic!("cleanup quarantine must return one timeout response");
    };
    assert_eq!(response.status(), 504);
    assert!(started.elapsed() >= Duration::from_millis(4_500));
    assert_eq!(completion.calls(), 0);
    assert!(bytes.used() > 0);
    drop_gate.release();
    drain(&runtime, guard);
    assert_eq!(completion.calls(), 1);
    assert_eq!(completion.outcomes(), [ResponseCompletionOutcome::TimedOut]);
    assert_eq!(completion.source_drops_at_call(), [1]);
    assert_eq!(
        signals.completed(),
        [(LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"))]
    );
    assert!(bytes.used() > 0);
    drop(response);
    assert_eq!(bytes.used(), 0);
}
