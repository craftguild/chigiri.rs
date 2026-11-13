//! Actual conversion faults are private and retain failed physical disposal.

mod application;

use std::process::Command;

use super::*;
use application::{Mode, Probe, Upgrade, fixture};

const CHILD: &str = "CHIGIRI_NATIVE_UPGRADE_POISON_CHILD";
const TEST: &str = "subscription::driver::tests::http_worker::managed::websocket::opening::failures::failed_conversion_disposal_keeps_the_original_connection_after_join";

#[test]
fn ordinary_conversion_disconnect_keeps_endpoint_admission_open() {
    let runtime = runtime();
    for mode in [Mode::Transport, Mode::Protocol] {
        let (endpoint, probe) = fixture(mode, State::new(Vec::new()));
        run(&runtime, async {
            let (service, upgrade) = managed_socket::admit(&endpoint).await;
            endpoint
                .__attach_managed_websocket(
                    upgrade,
                    Box::pin(Upgrade::new(probe.clone(), mode)),
                    &TIMERS,
                )
                .unwrap();
            until(&probe.drops, 1).await;
            released(&endpoint).await;
            assert_eq!(count(&probe.errors), 0);
            assert_eq!(endpoint.shutdown_outcome(), None);
            drop(service.request().unwrap());
            endpoint
                .shutdown_on(async {}, ShutdownOptions::graceful())
                .await;
            assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Drained));
            no_operation(&probe);
        });
    }
}

#[test]
fn conversion_invariant_and_panics_are_one_private_infrastructure_failure() {
    let runtime = runtime();
    for mode in [Mode::Invariant, Mode::Panic, Mode::Observer] {
        for stop_first in [false, true] {
            exercise(&runtime, mode, stop_first);
        }
    }
}

#[test]
fn failed_conversion_disposal_keeps_the_original_connection_after_join() {
    let Ok(case) = std::env::var(CHILD) else {
        for mode in ["payload", "pending", "ready"] {
            for stop_first in [false, true] {
                let output = Command::new(std::env::current_exe().unwrap())
                    .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
                    .env(CHILD, format!("{mode}:{stop_first}"))
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "owned conversion {mode}:{stop_first} child failed: {}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        return;
    };
    let (mode, stop_first) = case.split_once(':').unwrap();
    let mode = match mode {
        "payload" => Mode::Payload,
        "pending" => Mode::PendingDrop,
        "ready" => Mode::ReadyDrop,
        _ => unreachable!(),
    };
    exercise(&runtime(), mode, stop_first == "true");
}

async fn released(endpoint: &Endpoint<AppContext>) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while endpoint.subscription_connection_counts_for_test().0 != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("successful conversion disposal must release its original generation");
}

fn no_operation(probe: &Probe) {
    assert_eq!(count(&probe.calls), 1);
    assert_eq!(count(&probe.drops), 1);
    assert!(probe.disposed_in_owner.load(Ordering::SeqCst));
    assert_eq!(count(&probe.app.counts.factory_calls), 0);
    assert_eq!(count(&probe.app.counts.setup_calls), 0);
    assert_eq!(count(&probe.app.counts.completion_calls), 0);
    assert!(probe.peer.messages().is_empty());
}

fn exercise(runtime: &tokio::runtime::Runtime, mode: Mode, stop_first: bool) {
    let app = State::new(Vec::new());
    let release = Release(app.block_scalar());
    let (endpoint, probe) = fixture(mode, app);
    run(runtime, async {
        let (service, upgrade) = managed_socket::admit(&endpoint).await;
        endpoint
            .__attach_managed_websocket(
                upgrade,
                Box::pin(Upgrade::new(probe.clone(), mode)),
                &TIMERS,
            )
            .unwrap();
        until(&probe.calls, 1).await;
        assert!(release.0.wait_entered(Duration::from_secs(2)));
        let mut hook = if stop_first {
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
        if matches!(mode, Mode::PendingDrop) && hook.is_none() {
            until(&probe.pending, 1).await;
            let mut stop = endpoint.shutdown_on(async {}, ShutdownOptions::graceful());
            assert!(
                poll_fn(|cx| Poll::Ready(stop.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            hook = Some(stop);
        }
        until(&probe.errors, 1).await;
        hook.unwrap_or_else(|| endpoint.shutdown_on(async {}, ShutdownOptions::graceful()))
            .await;
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        assert_eq!(count(&probe.errors), 1);
        if mode.poisoned() {
            tokio::time::timeout(Duration::from_secs(2), async {
                while !crate::subscription::worker::process_poisoned_for_test() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap_or_else(|error| { panic!("actual native join must preserve the original poisoned record; error={error}, {}", crate::subscription::worker::process_diagnostic_for_test()) });
            assert_eq!(endpoint.subscription_connection_counts_for_test().0, 1);
        } else {
            released(&endpoint).await;
        }
        assert!(probe.observed_in_owner.load(Ordering::SeqCst));
        assert_eq!(endpoint.subscription_physical_counts_for_test().0, 0);
        assert!(service.request().is_err());
        no_operation(&probe);
    });
}
