//! Opaque authorization errors never become native join outputs or reaper work.

mod application;
mod diagnostics;

use super::*;
use application::{Mode, Probe, fixture};

const CHILD: &str = "CHIGIRI_MANAGED_AUTHORIZATION_POISON_CHILD";
const TEST: &str = "subscription::driver::tests::http_worker::managed::websocket::authorization::disposal::failed_authorization_disposal_keeps_the_original_connection_after_actual_join";

async fn released(endpoint: &Endpoint<AppContext>) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while endpoint.subscription_connection_counts_for_test().0 != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("authorization native join must release the original connection");
}

fn no_operation(probe: &Probe) {
    assert_eq!(count(&probe.calls), 1);
    assert_eq!(count(&probe.app.counts.factory_calls), 0);
    assert_eq!(count(&probe.app.counts.setup_calls), 0);
    assert_eq!(count(&probe.app.counts.completion_calls), 0);
}

#[test]
fn ordinary_authorization_reject_and_error_close_only_the_connection() {
    let runtime = runtime();
    for mode in [Mode::Reject, Mode::Error] {
        let (endpoint, probe) = fixture(mode, State::new(Vec::new()));
        run(&runtime, async {
            let managed = managed_socket::begin(&endpoint).await;
            released(&endpoint).await;
            no_operation(&probe);
            let rejected = matches!(mode, Mode::Reject);
            assert_eq!(count(&probe.errors), usize::from(!rejected));
            assert_eq!(count(&probe.drops), usize::from(!rejected));
            if !rejected {
                assert!(probe.observed_in_owner.load(Ordering::SeqCst));
                assert!(probe.disposed_in_owner.load(Ordering::SeqCst));
            }
            assert_eq!(
                managed.peer.closes(),
                [Some(if rejected { 4403 } else { 1011 })]
            );
            assert!(managed.peer.messages().is_empty());
            assert_eq!(endpoint.shutdown_outcome(), None);
            drop(managed.service.request().unwrap());
            endpoint
                .shutdown_on(async {}, ShutdownOptions::graceful())
                .await;
            assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Drained));
        });
    }
}

#[test]
fn authorization_observer_panic_is_one_attempt_in_the_original_task() {
    let (endpoint, probe) = fixture(Mode::Observer, State::new(Vec::new()));
    run(&runtime(), async {
        let managed = managed_socket::begin(&endpoint).await;
        until(&probe.errors, 1).await;
        endpoint
            .shutdown_on(async {}, ShutdownOptions::graceful())
            .await;
        released(&endpoint).await;
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        assert_eq!(count(&probe.errors), 1);
        assert_eq!(count(&probe.drops), 1);
        assert!(probe.observed_in_owner.load(Ordering::SeqCst));
        assert!(probe.disposed_in_owner.load(Ordering::SeqCst));
        assert!(managed.peer.messages().is_empty());
        assert!(managed.service.request().is_err());
        no_operation(&probe);
    });
}

#[test]
fn scoped_expiry_does_not_wait_again_or_release_a_blocked_authorization_error() {
    let (endpoint, probe) = fixture(Mode::Error, State::new(Vec::new()));
    let drop_probe = State::new(Vec::new());
    let release = Release(drop_probe.block_scalar());
    *probe.blocking_drop.lock().unwrap() = Some(drop_probe);
    run(&runtime(), async {
        let managed = managed_socket::begin(&endpoint).await;
        until(&probe.errors, 1).await;
        assert!(release.0.wait_entered(Duration::from_secs(2)));
        assert!(probe.observed_in_owner.load(Ordering::SeqCst));
        assert!(probe.disposed_in_owner.load(Ordering::SeqCst));
        tokio::time::timeout(
            Duration::from_secs(1),
            endpoint.shutdown_on(
                async {},
                ShutdownOptions::graceful_for(Duration::from_millis(20)).unwrap(),
            ),
        )
        .await
        .expect("the original scoped deadline cannot gain another cleanup wait");
        assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Expired));
        assert_eq!(endpoint.subscription_connection_counts_for_test().0, 1);
        assert!(managed.service.request().is_err());
        release.0.release();
        released(&endpoint).await;
        assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Expired));
        assert_eq!(count(&probe.errors), 1);
        assert_eq!(count(&probe.drops), 1);
        assert!(managed.peer.messages().is_empty());
        no_operation(&probe);
    });
}

#[test]
fn failed_authorization_disposal_keeps_the_original_connection_after_actual_join() {
    let Ok(case) = std::env::var(CHILD) else {
        for mode in ["error", "both", "payload", "pending", "ready"] {
            for stop_first in [false, true] {
                let output = diagnostics::child(&format!("{mode}:{stop_first}"), "bounded");
                assert!(
                    output.status.success(),
                    "owned authorization {mode}:{stop_first} child failed: {}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        return;
    };
    let (mode, stop_first) = case.split_once(':').unwrap();
    let panics = diagnostics::observe_panics();
    let mode = match mode {
        "error" => Mode::ErrorDrop,
        "both" => Mode::Both,
        "payload" => Mode::Payload,
        "pending" => Mode::PendingFuture,
        "ready" => Mode::ReadyFuture,
        _ => unreachable!(),
    };
    let app = State::new(Vec::new());
    let release = Release(app.block_scalar());
    let (endpoint, probe) = fixture(mode, app);
    let evidence = diagnostics::Evidence::new(&endpoint, &probe, &case);
    run(&runtime(), async {
        evidence.at("managed opening");
        let managed = managed_socket::begin(&endpoint).await;
        evidence.at("authorization entry");
        until(&probe.calls, 1).await;
        assert!(release.0.wait_entered(Duration::from_secs(2)));
        let mut hook = if stop_first == "true" {
            let mut hook = endpoint.shutdown_on(async {}, ShutdownOptions::graceful());
            assert!(
                poll_fn(|cx| Poll::Ready(hook.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            Some(hook)
        } else {
            None
        };
        release.0.release();
        if matches!(mode, Mode::PendingFuture) && hook.is_none() {
            tokio::time::timeout(Duration::from_secs(2), async {
                while !probe.pending.load(Ordering::SeqCst) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let mut stop = endpoint.shutdown_on(async {}, ShutdownOptions::graceful());
            assert!(
                poll_fn(|cx| Poll::Ready(stop.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            hook = Some(stop);
        }
        evidence.at("authorization observation");
        until(&probe.errors, 1).await;
        evidence.at("shutdown completion");
        hook.unwrap_or_else(|| endpoint.shutdown_on(async {}, ShutdownOptions::graceful()))
            .await;
        evidence.at("actual native join");
        tokio::time::timeout(Duration::from_secs(2), async {
            while !crate::subscription::worker::process_poisoned_for_test() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|error| {
            panic!("actual authorization native JoinReady must select its poisoned record; error={error}, {}", crate::subscription::worker::process_diagnostic_for_test())
        });
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        assert_eq!(count(&probe.errors), 1);
        assert_eq!(count(&probe.drops), 1);
        assert!(probe.observed_in_owner.load(Ordering::SeqCst));
        assert!(probe.disposed_in_owner.load(Ordering::SeqCst));
        assert_eq!(endpoint.subscription_connection_counts_for_test().0, 1);
        assert_eq!(endpoint.subscription_physical_counts_for_test().0, 0);
        assert!(managed.peer.messages().is_empty());
        assert!(managed.service.request().is_err());
        no_operation(&probe);
        assert_eq!(
            count(&panics),
            if matches!(mode, Mode::Both | Mode::Payload) {
                2
            } else {
                1
            }
        );
        evidence.complete();
    });
}

#[test]
fn authorization_fixture_deadline_detects_blocked_panic_diagnostics() {
    let delayed = diagnostics::child("both:false", "delay");
    assert!(!delayed.status.success());
    let stderr = String::from_utf8_lossy(&delayed.stderr);
    assert!(stderr.contains("authorization diagnostic delay probe entered"));
    assert!(stderr.contains("authorization fixture incomplete: phase=shutdown completion"));
    let bounded = diagnostics::child("both:false", "bounded");
    assert!(
        bounded.status.success(),
        "bounded authorization diagnostics failed: {}\n{}",
        String::from_utf8_lossy(&bounded.stdout),
        String::from_utf8_lossy(&bounded.stderr)
    );
}
