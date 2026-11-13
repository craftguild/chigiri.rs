//! Normative first-notification, deadline, event-start, and outcome contracts.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use super::{Control, StopPhase};
use crate::managed::EventActivity;
use crate::{ShutdownOptions, ShutdownOutcome};

#[test]
fn first_notification_closes_admission_and_repetition_preserves_its_deadline() {
    let control = Control::new();
    control.activation_started().unwrap();
    let scope = control.admit().unwrap();
    let now = tokio::time::Instant::now();
    let options = ShutdownOptions::graceful_for(Duration::from_secs(2)).unwrap();
    let expected = StopPhase::Graceful {
        deadline: now + Duration::from_secs(2),
    };
    assert_eq!(control.begin(options, now), expected);
    assert!(control.admit().is_err());
    assert!(control.registration_gate().is_err());
    assert!(control.emission_gate().is_ok());
    assert_eq!(
        control.begin(ShutdownOptions::immediate(), now + Duration::from_secs(1)),
        expected
    );
    assert_eq!(
        control.begin(ShutdownOptions::graceful(), now + Duration::from_secs(1)),
        expected
    );
    assert_eq!(control.outcome(), None);
    assert!(!control.requests_idle());
    drop(scope);
    control.drained(now + Duration::from_secs(1));
    assert_eq!(control.outcome(), Some(ShutdownOutcome::Drained));
    assert!(control.emission_gate().is_err());
}

#[test]
fn expiry_wins_equal_completion_and_late_join_never_rewrites_it() {
    let control = Control::new();
    control.activation_started().unwrap();
    let now = tokio::time::Instant::now();
    let StopPhase::Graceful { deadline } = control.begin(ShutdownOptions::graceful(), now) else {
        panic!("an activated endpoint has a graceful phase");
    };
    assert_eq!(deadline - now, Duration::from_secs(30));
    control.expire(deadline - Duration::from_nanos(1));
    assert_eq!(control.outcome(), None);
    control.drained(deadline);
    assert_eq!(control.outcome(), Some(ShutdownOutcome::Expired));
    control.drained(deadline + Duration::from_secs(1));
    control.fail();
    assert_eq!(control.outcome(), Some(ShutdownOutcome::Expired));
}

#[test]
fn infrastructure_failure_cannot_be_rewritten_as_a_successful_drain() {
    let control = Control::new();
    control.activation_started().unwrap();
    let now = tokio::time::Instant::now();
    control.begin(ShutdownOptions::graceful(), now);
    control.fail();
    control.drained(now);
    assert_eq!(
        control.outcome(),
        Some(ShutdownOutcome::InfrastructureFailure)
    );
    assert_eq!(control.phase(), StopPhase::Cancelled);
}

#[test]
fn owner_abandonment_cancels_a_pending_drain_without_reporting_success() {
    let control = Control::new();
    control.activation_started().unwrap();
    let now = tokio::time::Instant::now();
    control.begin(ShutdownOptions::graceful(), now);
    control.stop();
    control.drained(now);
    assert_eq!(control.phase(), StopPhase::Cancelled);
    assert_eq!(
        control.outcome(),
        Some(ShutdownOutcome::CancellationRequested)
    );
}

#[test]
fn event_start_and_shutdown_are_ordered_without_polling_application_work_under_a_lock() {
    let control = Control::new();
    control.activation_started().unwrap();
    let active = EventActivity::new(Some(Arc::downgrade(&control)));
    let pending = EventActivity::new(Some(Arc::downgrade(&control)));
    assert!(active.start());
    control.begin(ShutdownOptions::graceful(), tokio::time::Instant::now());
    assert!(
        active.is_active(),
        "already admitted context/execution may finish"
    );
    assert!(
        !pending.start(),
        "a pending source cannot create a fresh event context"
    );
    active.finish();
    assert!(!active.is_active());
    assert!(!active.start());
}

#[test]
fn concurrent_source_ready_and_shutdown_never_start_an_event_after_closed_admission() {
    for _ in 0..8 {
        let control = Control::new();
        control.activation_started().unwrap();
        let activity = EventActivity::new(Some(Arc::downgrade(&control)));
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let starting = activity.clone();
        let start_barrier = barrier.clone();
        let start = std::thread::spawn(move || {
            start_barrier.wait();
            starting.start()
        });
        let stopping = control.clone();
        let stop = std::thread::spawn(move || {
            barrier.wait();
            stopping.begin(ShutdownOptions::graceful(), tokio::time::Instant::now());
        });
        let admitted = start.join().unwrap();
        stop.join().unwrap();
        assert_eq!(activity.is_active(), admitted);
        assert!(control.registration_gate().is_err());
        activity.finish();
        assert!(!activity.start());
    }
}

#[test]
fn emission_authority_expires_even_when_the_supervisor_has_not_polled() {
    let control = Control::new();
    control.activation_started().unwrap();
    let prior = tokio::time::Instant::now() - Duration::from_secs(1);
    let options = ShutdownOptions::graceful_for(Duration::from_millis(1)).unwrap();
    control.begin(options, prior);
    assert!(control.emission_gate().is_err());
    assert_eq!(control.outcome(), Some(ShutdownOutcome::Expired));
}

#[test]
fn immediate_notification_does_not_wait_for_an_outstanding_scope() {
    let control = Control::new();
    let scope = control.admit().unwrap();
    let mut hook = control.shutdown_on(async {}, ShutdownOptions::immediate());
    assert_eq!(
        hook.as_mut().poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(())
    );
    assert!(!control.requests_idle());
    assert_eq!(
        control.outcome(),
        Some(ShutdownOutcome::CancellationRequested)
    );
    drop(scope);
}

#[test]
fn graceful_without_activated_work_drains_without_a_runtime_or_timer() {
    let control = Control::new();
    let mut hook = control.shutdown_on(async {}, ShutdownOptions::graceful());
    assert_eq!(
        Pin::new(&mut hook).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(())
    );
    assert_eq!(control.outcome(), Some(ShutdownOutcome::Drained));
}

#[test]
fn a_native_hook_returns_on_expiry_or_classified_failure() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let control = Control::new();
        control.activation_started().unwrap();
        tokio::time::timeout(
            Duration::from_secs(1),
            control.shutdown_on(
                async {},
                ShutdownOptions::graceful_for(Duration::from_millis(2)).unwrap(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(control.outcome(), Some(ShutdownOutcome::Expired));
        let failed = Control::new();
        failed.activation_started().unwrap();
        let mut hook = failed.shutdown_on(async {}, ShutdownOptions::graceful());
        assert!(
            std::future::poll_fn(|cx| Poll::Ready(hook.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        failed.fail();
        tokio::time::timeout(Duration::from_secs(1), hook)
            .await
            .unwrap();
        assert_eq!(
            failed.outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
    });
}
