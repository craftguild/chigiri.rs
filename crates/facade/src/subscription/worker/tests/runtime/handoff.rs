//! Closed registration recovery must not depend on dropping its endpoint graph.

use super::*;

#[test]
fn detached_registration_collects_existing_and_late_worker_quarantine_without_app_polling() {
    for late_commit in [false, true] {
        let runtime = runtime();
        let gate = BlockingGate::new();
        let owner = owner();
        let guard = owner.register_worker_runtime().unwrap();
        let cleanup = owner.cleanup_driver();
        let bytes = owner.delivery().byte_owner();
        let reservation = bytes.try_reserve(EVENT_BYTES).unwrap();
        let active = owner
            .begin_http_subscription()
            .unwrap()
            .activate(BridgeOperationKind::Subscription)
            .unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let worker = {
            let _entered = runtime.enter();
            active.spawn_worker(gate.task(entered_tx)).unwrap()
        };
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let terminal = active.revoke().unwrap();
        let deadlines = crate::subscription::worker::MonitoredDeadlineTable::with_overall(
            Box::pin(async move {
                drop(reservation);
                std::future::pending::<()>().await;
            }),
        )
        .unwrap();
        let retained = WorkerRetainedState::orphan_active(Some(deadlines), None, None);
        let detached = DetachedCleanup::worker(worker.erase(), retained);
        if late_commit {
            drop(guard.shutdown().unwrap());
            terminal.quarantine(detached).unwrap();
        } else {
            terminal.quarantine(detached).unwrap();
            drop(guard);
        }
        assert_eq!(
            cleanup.pending(),
            Ok(0),
            "the completion now belongs to its process slot"
        );
        assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 1);
        assert_eq!(bytes.used(), EVENT_BYTES);
        gate.release();
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                while owner.delivery().snapshot().endpoint_active_subscriptions != 0
                    || bytes.used() != 0
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("registered process recovery must join while Endpoint resources remain alive");
        });
        assert_eq!(cleanup.pending(), Ok(0));
        let replacement = owner.register_worker_runtime().unwrap();
        runtime.block_on(replacement.shutdown().unwrap()).unwrap();
    }
}

const POISON_CHILD: &str = "CHIGIRI_NATIVE_POISON_HANDOFF_CHILD";
const POISON_TEST: &str = "subscription::worker::tests::runtime::handoff::poisoned_quarantine_reaches_the_process_owner_when_registration_closes";

struct PoisonPayload {
    detach: Option<Arc<Mutex<Option<crate::subscription::worker::SubscriptionRuntimeGuard>>>>,
}

impl Drop for PoisonPayload {
    fn drop(&mut self) {
        if let Some(detach) = self.detach.take() {
            let guard = detach.lock().unwrap().take();
            drop(guard);
        }
        panic!("owned native panic-payload disposal failure");
    }
}

#[test]
fn poisoned_quarantine_reaches_the_process_owner_when_registration_closes() {
    let Some(mode) = std::env::var_os(POISON_CHILD) else {
        for mode in ["after-poison", "during-poll"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([POISON_TEST, "--exact", "--nocapture", "--test-threads=1"])
                .env(POISON_CHILD, mode)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "owned native poison handoff {mode} failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    };
    let during_poll = match mode.to_str().unwrap() {
        "after-poison" => false,
        "during-poll" => true,
        _ => panic!("unknown owned child mode"),
    };
    let runtime = runtime();
    let owner = owner();
    let guard = Arc::new(Mutex::new(Some(owner.register_worker_runtime().unwrap())));
    let cleanup = owner.cleanup_driver();
    let bytes = owner.delivery().byte_owner();
    let reservation = bytes.try_reserve(EVENT_BYTES).unwrap();
    let active = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let payload = PoisonPayload {
        detach: during_poll.then(|| Arc::clone(&guard)),
    };
    let task: BoxFuture<'static, ()> = Box::pin(async move {
        std::panic::panic_any(payload);
    });
    let worker = {
        let _entered = runtime.enter();
        active.spawn_worker(task).unwrap()
    };
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !worker.is_finished_for_test() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the owned native task must reach actual JoinReady");
    });
    let deadlines =
        crate::subscription::worker::MonitoredDeadlineTable::with_overall(Box::pin(async move {
            std::future::pending::<()>().await;
            drop(reservation);
        }))
        .unwrap();
    active
        .revoke()
        .unwrap()
        .quarantine(DetachedCleanup::worker(
            worker.erase(),
            WorkerRetainedState::orphan_active(Some(deadlines), None, None),
        ))
        .unwrap();
    assert!(matches!(poll_cleanup(&cleanup), Poll::Ready(Err(_))));
    if !during_poll {
        let guard = guard.lock().unwrap().take();
        drop(guard);
    }
    assert!(guard.lock().unwrap().is_none());
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !crate::subscription::worker::process_poisoned_for_test() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|error| {
            panic!(
                "closed registration must transfer the original poisoned native record; mode={mode:?}, error={error}, {}",
                crate::subscription::worker::process_diagnostic_for_test()
            );
        });
    });
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 1);
    assert_eq!(bytes.used(), EVENT_BYTES);
    assert!(cleanup.pending().is_err());
}
