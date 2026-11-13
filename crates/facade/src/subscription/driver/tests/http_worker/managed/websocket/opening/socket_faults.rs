//! Native faults retain the original connection and SDK-supervisor join.

mod application;

use super::*;
use application::{Fault, Method, Probe, fixture, native};
use std::process::Command;

const CHILD: &str = "CHIGIRI_NATIVE_SOCKET_POISON_CHILD";
const TEST: &str = "subscription::driver::tests::http_worker::managed::websocket::opening::socket_faults::failed_native_socket_disposal_retains_original_owners_after_supervisor_join";

#[test]
fn every_native_io_role_guards_panics_and_invariants_in_the_sdk_task() {
    let runtime = runtime();
    for active in [false, true] {
        for method in [
            Method::Receive,
            Method::Ready,
            Method::Start,
            Method::Write,
            Method::Close,
        ] {
            for fault in [Fault::Panic, Fault::Invariant] {
                exercise(&runtime, method, fault, active);
            }
        }
    }
}

#[test]
fn failed_native_socket_disposal_retains_original_owners_after_supervisor_join() {
    let Ok(case) = std::env::var(CHILD) else {
        for fault in ["payload", "drop", "observer", "ack", "constructor"] {
            for active in [false, true] {
                if fault == "ack" && active {
                    continue;
                }
                let output = Command::new(std::env::current_exe().unwrap())
                    .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
                    .env(CHILD, format!("{fault}:{active}"))
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "owned socket {fault}:{active} child failed: {}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        return;
    };
    let (fault, active) = case.split_once(':').unwrap();
    let (method, fault) = match fault {
        "payload" => (Method::Receive, Fault::Payload),
        "drop" => (Method::Drop, Fault::Panic),
        "observer" => (Method::Drop, Fault::Observer),
        "ack" => (Method::Drop, Fault::AckDrop),
        "constructor" => (Method::Drop, Fault::ConstructorDrop),
        _ => unreachable!(),
    };
    exercise(&runtime(), method, fault, active == "true");
}

fn exercise(runtime: &tokio::runtime::Runtime, method: Method, fault: Fault, active: bool) {
    let (endpoint, probe) = fixture(fault);
    run(runtime, async {
        let (service, upgrade) = managed_socket::admit(&endpoint).await;
        let ack = matches!(fault, Fault::AckDrop);
        if ack {
            probe.peer.stall_flush();
        }
        if active || ack {
            probe.peer.text(r#"{"type":"connection_init"}"#);
        }
        endpoint
            .__attach_managed_websocket(
                upgrade,
                Box::pin(std::future::ready(Ok(native(probe.clone(), method, fault)))),
                if matches!(fault, Fault::ConstructorDrop) {
                    &application::FAIL_CLEANUP_TIMER
                } else {
                    &TIMERS
                },
            )
            .unwrap();
        if ack {
            tokio::time::timeout(Duration::from_secs(2), async {
                while !probe.peer.has_current_frame() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("the original unflushed ACK must still own its physical frame");
        } else if active {
            tokio::time::timeout(Duration::from_secs(2), async {
                while probe.peer.messages().is_empty() || probe.peer.has_current_frame() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("native fixture must reach the flushed ACK before its active fault");
            assert_eq!(probe.peer.messages(), [r#"{"type":"connection_ack"}"#]);
        }
        probe.armed.store(true, Ordering::SeqCst);
        if ack {
            endpoint
                .shutdown_on(async {}, ShutdownOptions::graceful())
                .await;
        } else if matches!(method, Method::Close | Method::Drop) {
            probe.peer.text("invalid JSON");
        } else if active {
            probe.peer.text(r#"{"type":"ping"}"#);
        } else {
            probe.peer.text(r#"{"type":"connection_init"}"#);
        }
        until(&probe.errors, 1).await;
        endpoint
            .shutdown_on(async {}, ShutdownOptions::graceful())
            .await;
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        assert_eq!(count(&probe.errors), 1);
        assert_eq!(count(&probe.drops), 1);
        assert!(service.request().is_err());
        let poisoned = matches!(method, Method::Drop) || matches!(fault, Fault::Payload);
        if poisoned {
            let control = endpoint.managed_control();
            tokio::time::timeout(Duration::from_secs(2), async {
                while !crate::subscription::worker::managed_supervisor_poisoned_for_test(&control)
                    || !crate::subscription::worker::process_poisoned_for_test()
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap_or_else(|error| { panic!("the original native SDK-supervisor join must select its poisoned record; error={error}, {}", crate::subscription::worker::process_diagnostic_for_test()) });
            assert!(crate::subscription::worker::process_poisoned_for_test());
            assert_eq!(
                crate::subscription::worker::managed_socket_tickets_for_test(&control),
                1,
                "the original connection cleanup ticket remains with its physical owner"
            );
            assert_eq!(endpoint.subscription_connection_counts_for_test().0, 1);
        } else {
            tokio::time::timeout(Duration::from_secs(2), async {
                while endpoint.subscription_connection_counts_for_test().0 != 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("successful native disposal releases the original ticket and connection");
        }
        assert!(probe.observed_in_owner.load(Ordering::SeqCst));
        assert!(probe.dropped_in_owner.load(Ordering::SeqCst));
        no_operation(&probe);
        if poisoned {
            let control = endpoint.managed_control();
            let application = Arc::downgrade(&probe);
            drop(service);
            drop(endpoint);
            drop(probe);
            tokio::time::timeout(Duration::from_secs(2), async {
                while application.upgrade().is_some() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect(
                "poisoned supervisor recovery retains closed metadata, not the application graph",
            );
            assert!(crate::subscription::worker::managed_supervisor_poisoned_for_test(&control));
            assert_eq!(
                crate::subscription::worker::managed_socket_tickets_for_test(&control),
                1
            );
        }
    });
}

fn no_operation(probe: &Probe) {
    assert_eq!(count(&probe.app.counts.factory_calls), 0);
    assert_eq!(count(&probe.app.counts.setup_calls), 0);
    assert_eq!(count(&probe.app.counts.completion_calls), 0);
    for frame in probe.peer.messages() {
        assert!(!frame.contains("private"));
        assert!(!frame.contains(r#""type":"next""#));
    }
}

#[test]
fn ordinary_native_socket_disconnect_closes_only_its_connection() {
    let runtime = runtime();
    for active in [false, true] {
        let (endpoint, probe) = fixture(Fault::Transport);
        run(&runtime, async {
            let (service, upgrade) = managed_socket::admit(&endpoint).await;
            if active {
                probe.peer.text(r#"{"type":"connection_init"}"#);
            }
            endpoint
                .__attach_managed_websocket(
                    upgrade,
                    Box::pin(std::future::ready(Ok(native(
                        probe.clone(),
                        Method::Receive,
                        Fault::Transport,
                    )))),
                    &TIMERS,
                )
                .unwrap();
            if active {
                tokio::time::timeout(Duration::from_secs(2), async {
                    while probe.peer.messages().is_empty() || probe.peer.has_current_frame() {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("ordinary disconnect fixture waits for its flushed ACK");
            }
            probe.armed.store(true, Ordering::SeqCst);
            probe.peer.text(r#"{"type":"ping"}"#);
            until(&probe.drops, 1).await;
            tokio::time::timeout(Duration::from_secs(2), async {
                while endpoint.subscription_connection_counts_for_test().0 != 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("ordinary native disconnect releases the original connection");
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
