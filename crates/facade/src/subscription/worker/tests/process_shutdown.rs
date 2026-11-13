//! Isolated proof for the irreversible process-level reaper shutdown.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use chigiri_runtime::SubscriptionOverflow;

use super::super::process::SubscriptionProcessShutdown;
use super::super::process::reaper_thread_counts;
use super::super::test_waker::{ProbeCounts, probing_waker};
use super::super::{WorkerRetainedState, shutdown_subscription_runtime_process};
use crate::subscription::bridge_resources::{
    BridgeOperationKind, BridgeResourceOwner, DetachedCleanup,
};
use crate::subscription::outbox::SubscriptionOutbox;
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};

const CHILD_ENV: &str = "CHIGIRI_PROCESS_SHUTDOWN_CHILD";
const EVENT_BYTES: usize = 1024;

struct BlockingGate {
    state: Arc<(Mutex<bool>, Condvar)>,
}

impl BlockingGate {
    fn new() -> Self {
        Self {
            state: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    fn task(
        &self,
        entered: std::sync::mpsc::SyncSender<()>,
    ) -> chigiri_http::BoxFuture<'static, ()> {
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            entered.send(()).unwrap();
            let (lock, signal) = &*state;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = signal.wait(released).unwrap();
            }
        })
    }

    fn release(&self) {
        let (lock, signal) = &*self.state;
        *lock.lock().unwrap() = true;
        signal.notify_all();
    }
}

impl Drop for BlockingGate {
    fn drop(&mut self) {
        self.release();
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .unwrap()
}

fn settings() -> ValidatedSubscriptionSettings {
    ValidatedSubscriptionSettings::new(
        SubscriptionLimits {
            max_active_subscriptions: 1,
            max_pending_operation_setups: 2,
            max_encoded_event_bytes: EVENT_BYTES,
            max_endpoint_encoded_bytes: EVENT_BYTES,
            ..SubscriptionLimits::default()
        },
        SubscriptionProtocolSettings::default(),
    )
}

fn reserve_event(
    owner: &BridgeResourceOwner,
) -> crate::subscription::outbox::SubscriptionEventReservation {
    SubscriptionOutbox::new(
        &settings(),
        owner.delivery().byte_owner(),
        SubscriptionOverflow::Backpressure,
        chigiri_http::SubscriptionTransport::ServerSentEvents,
        None,
    )
    .unwrap()
    .reserve_before_poll(0)
    .unwrap()
}

fn poll_pending<F>(future: &mut F)
where
    F: Future + Unpin,
{
    assert!(matches!(
        Pin::new(future).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
}

fn run_isolated_child() {
    let first_runtime = runtime();
    let second_runtime = runtime();
    let gate = BlockingGate::new();
    let first = BridgeResourceOwner::new(&settings()).unwrap();
    let second = BridgeResourceOwner::new(&settings()).unwrap();
    let first_bytes = first.delivery().byte_owner();
    let second_bytes = second.delivery().byte_owner();
    let first_guard = first.register_worker_runtime().unwrap();
    let second_guard = second.register_worker_runtime().unwrap();

    let first_active = first
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let first_reservation = reserve_event(&first);
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let first_worker = {
        let _entered = first_runtime.enter();
        first_active.spawn_worker(gate.task(entered_tx)).unwrap()
    };
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    first_active
        .revoke()
        .unwrap()
        .quarantine(DetachedCleanup::worker(
            first_worker.erase(),
            WorkerRetainedState::orphan_active(None, Some(first_reservation), None),
        ))
        .unwrap();

    let second_active = second
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let second_reservation = reserve_event(&second);
    let second_worker = {
        let _entered = second_runtime.enter();
        second_active.spawn_worker(async {}).unwrap()
    };
    second_active
        .revoke()
        .unwrap()
        .quarantine(DetachedCleanup::worker(
            second_worker.erase(),
            WorkerRetainedState::orphan_active(None, Some(second_reservation), None),
        ))
        .unwrap();

    let mut shutdown = shutdown_subscription_runtime_process().unwrap();
    assert!(shutdown_subscription_runtime_process().is_err());
    let rejected = BridgeResourceOwner::new(&settings()).unwrap();
    assert!(rejected.register_worker_runtime().is_err());
    poll_pending(&mut shutdown);
    drop(shutdown);

    let unwind = catch_unwind(AssertUnwindSafe(|| {
        let mut resumed = shutdown_subscription_runtime_process().unwrap();
        assert!(shutdown_subscription_runtime_process().is_err());
        poll_pending(&mut resumed);
        panic!("exercise shutdown-owner unwind");
    }));
    assert!(unwind.is_err());
    let stopped_owner = shutdown_subscription_runtime_process().unwrap();

    assert_eq!(first_bytes.used(), EVENT_BYTES);
    assert_eq!(second_bytes.used(), EVENT_BYTES);
    drop(first_guard);
    drop(second_guard);
    drop(first);
    drop(second);
    drop(rejected);
    gate.release();

    let wait_started = Instant::now();
    while reaper_thread_counts().1 == 0 {
        assert!(
            wait_started.elapsed() < Duration::from_secs(2),
            "process reaper did not stop"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(reaper_thread_counts(), (1, 1, 0));
    drop(stopped_owner);
    let final_shutdown = shutdown_subscription_runtime_process().unwrap();

    first_runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(2), final_shutdown).await })
        .expect("final process drain did not finish")
        .unwrap();
    assert_eq!(first_bytes.used(), 0);
    assert_eq!(second_bytes.used(), 0);
    assert_eq!(reaper_thread_counts(), (1, 1, 1));
    assert!(shutdown_subscription_runtime_process().is_err());
    assert!(
        BridgeResourceOwner::new(&settings())
            .unwrap()
            .register_worker_runtime()
            .is_err()
    );

    drop(first_runtime);
    drop(second_runtime);
}

#[test]
fn final_process_shutdown_is_recoverable_and_joins_once() {
    if std::env::var_os(CHILD_ENV).is_some() {
        run_isolated_child();
        return;
    }

    let output = Command::new(std::env::current_exe().unwrap())
        .arg("final_process_shutdown_is_recoverable_and_joins_once")
        .arg("--test-threads=1")
        .arg("--nocapture")
        .env(CHILD_ENV, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn process_shutdown_replaces_and_drops_wakers_after_unlocking() {
    let mut shutdown = SubscriptionProcessShutdown::pending_for_test(false);
    let lock = shutdown.lock_probe_for_test();
    let first_counts = ProbeCounts::new();
    let first = probing_waker(Arc::clone(&lock), first_counts.clone());
    assert_eq!(
        Pin::new(&mut shutdown).poll(&mut Context::from_waker(first.waker())),
        Poll::Pending
    );
    assert_eq!(first.strong_count(), 2);
    drop(first);

    let second_counts = ProbeCounts::new();
    let second = probing_waker(lock, second_counts.clone());
    assert_eq!(
        Pin::new(&mut shutdown).poll(&mut Context::from_waker(second.waker())),
        Poll::Pending
    );
    assert_eq!(first_counts.values(), (1, 0, 0));
    assert_eq!(second.strong_count(), 2);
    drop(second);
    drop(shutdown);
    assert_eq!(second_counts.values(), (1, 0, 0));
}

#[test]
fn process_shutdown_disposes_unused_and_joined_wakers_after_unlocking() {
    let mut invalid = SubscriptionProcessShutdown::pending_for_test(false);
    invalid.set_owner_for_test(false);
    let invalid_counts = ProbeCounts::new();
    let proposed = probing_waker(invalid.lock_probe_for_test(), invalid_counts.clone());
    assert_eq!(
        Pin::new(&mut invalid).poll(&mut Context::from_waker(proposed.waker())),
        Poll::Ready(Err(super::super::SubscriptionWorkerRuntimeError::Invariant))
    );
    drop(proposed);
    assert_eq!(invalid_counts.values(), (1, 0, 0));
    drop(invalid);

    let mut stopped = SubscriptionProcessShutdown::stopped_for_test();
    let stored_counts = ProbeCounts::new();
    let stored = probing_waker(stopped.lock_probe_for_test(), stored_counts.clone());
    stopped.set_waker_for_test(stored.waker().clone());
    drop(stored);
    assert_eq!(
        Pin::new(&mut stopped).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(()))
    );
    assert_eq!(stored_counts.values(), (1, 0, 0));
}

#[test]
fn poisoned_process_state_still_disposes_shutdown_wakers_after_unlocking() {
    let mut shutdown = SubscriptionProcessShutdown::pending_for_test(true);
    let counts = ProbeCounts::new();
    let proposed = probing_waker(shutdown.lock_probe_for_test(), counts.clone());
    assert_eq!(
        Pin::new(&mut shutdown).poll(&mut Context::from_waker(proposed.waker())),
        Poll::Pending
    );
    assert_eq!(proposed.strong_count(), 2);
    drop(proposed);
    drop(shutdown);
    assert_eq!(counts.values(), (1, 0, 0));
}
