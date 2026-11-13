//! Final registry-owner Drop aborts a task awaiting an application hook.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use chigiri_http::{ResponseCompletionOutcome, SubscriptionTransport};
use chigiri_runtime::LifecycleResult;

use super::super::application::{CompletionBehavior, CompletionProbe, install_completion};
use super::super::support::{
    DeadlineFixture, VALID, WakeLatch, endpoint, pending_with_deadlines, run, runtime,
};
use super::poll_until;
use crate::subscription::driver::tests::state::{
    EventMode, FactoryMode, HookMode, SetupMode, State,
};

#[test]
fn dropping_last_registry_owner_aborts_pending_hook_and_drains_its_exact_slot() {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::modes(
        Vec::new(),
        SetupMode::Ready,
        EventMode::Object,
        FactoryMode::Ready,
        FactoryMode::Ready,
        HookMode::Pending,
    );
    let (mut endpoint, signals) = endpoint(Arc::clone(&state));
    let completion = CompletionProbe::new(CompletionBehavior::Normal, None, false);
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
        state.counts.hook_calls.load(Ordering::SeqCst) == 1
    });
    assert_eq!(state.counts.setup_calls.load(Ordering::SeqCst), 1);
    let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
    assert!(bytes.used() > 0);
    drop(preparation);
    assert_eq!(signals.completed(), [(LifecycleResult::Cancelled, None)]);
    drop(endpoint);
    let shutdown = guard.shutdown().unwrap().into_registration_shutdown();
    runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(3), shutdown).await })
        .expect("aborted pending hook must reach exact registration drain")
        .unwrap();
    assert_eq!(state.counts.hook_future_drops.load(Ordering::SeqCst), 1);
    assert_eq!(state.counts.source_drops.load(Ordering::SeqCst), 1);
    assert_eq!(bytes.used(), 0);
    assert_eq!(completion.calls(), 1);
    assert_eq!(completion.outcomes(), [ResponseCompletionOutcome::Failed]);
    assert_eq!(completion.source_drops_at_call(), [1]);
}
