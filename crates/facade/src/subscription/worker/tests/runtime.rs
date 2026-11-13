use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use chigiri_http::{BoxFuture, DeadlineFactory, SubscriptionTransport};
use chigiri_runtime::SubscriptionOverflow;
use futures_core::Stream;

use super::super::WorkerRetainedState;
use crate::subscription::bridge_resources::{
    BridgeCleanupOutcome, BridgeOperationKind, BridgeResourceOwner, DetachedCleanup,
};
use crate::subscription::delivery::ByteReservationError;
use crate::subscription::outbox::SubscriptionOutbox;
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};

mod handoff;

struct TokioDeadlines;

impl DeadlineFactory for TokioDeadlines {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        let deadline = tokio::time::Instant::now().checked_add(duration);
        Box::pin(async move {
            if let Some(deadline) = deadline {
                tokio::time::sleep_until(deadline).await;
            }
        })
    }
}

static TOKIO_DEADLINES: TokioDeadlines = TokioDeadlines;
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

    fn task(&self, entered: std::sync::mpsc::SyncSender<()>) -> BoxFuture<'static, ()> {
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

struct BlockingSource {
    state: Arc<(Mutex<bool>, Condvar)>,
    entered: Option<std::sync::mpsc::SyncSender<()>>,
}

impl Stream for BlockingSource {
    type Item = ();

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(entered) = self.entered.take() {
            entered.send(()).unwrap();
        }
        let (lock, signal) = &*self.state;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = signal.wait(released).unwrap();
        }
        Poll::Ready(None)
    }
}

fn source_task(
    gate: &BlockingGate,
    entered: std::sync::mpsc::SyncSender<()>,
) -> BoxFuture<'static, ()> {
    let mut source = BlockingSource {
        state: Arc::clone(&gate.state),
        entered: Some(entered),
    };
    Box::pin(async move {
        std::future::poll_fn(|context| Pin::new(&mut source).poll_next(context)).await;
    })
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .unwrap()
}

fn settings() -> ValidatedSubscriptionSettings {
    let limits = SubscriptionLimits {
        max_active_subscriptions: 1,
        max_pending_operation_setups: 2,
        max_encoded_event_bytes: EVENT_BYTES,
        max_endpoint_encoded_bytes: EVENT_BYTES,
        ..SubscriptionLimits::default()
    };
    ValidatedSubscriptionSettings::new(limits, SubscriptionProtocolSettings::default())
}

fn owner() -> BridgeResourceOwner {
    BridgeResourceOwner::new(&settings()).unwrap()
}

fn poll_cleanup(
    driver: &crate::subscription::bridge_resources::BridgeCleanupDriver,
) -> Poll<Result<bool, crate::subscription::bridge_resources::BridgeCleanupError>> {
    driver.poll_one(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn ready_worker_completion_releases_without_entering_quarantine() {
    let runtime = runtime();
    let owner = owner();
    let guard = owner.register_worker_runtime().unwrap();
    let active = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let worker = {
        let _entered = runtime.enter();
        active.spawn_worker(async {}).unwrap()
    };
    let terminal = active.revoke().unwrap();
    let detached = DetachedCleanup::worker(worker.erase(), WorkerRetainedState::empty());

    assert_eq!(
        runtime.block_on(terminal.coordinate(detached, &TOKIO_DEADLINES)),
        BridgeCleanupOutcome::Completed
    );
    assert_eq!(owner.cleanup_driver().pending(), Ok(0));
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}

#[test]
fn synchronous_source_poll_stays_charged_through_the_real_five_second_quarantine() {
    let runtime = runtime();
    let gate = BlockingGate::new();
    let owner = owner();
    let guard = owner.register_worker_runtime().unwrap();
    let cleanup = owner.cleanup_driver();
    let bytes = owner.delivery().byte_owner();
    let mut outbox = SubscriptionOutbox::new(
        &settings(),
        bytes.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
        None,
    )
    .unwrap();
    let reservation = outbox.reserve_before_poll(0).unwrap();
    let active = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let worker = {
        let _entered = runtime.enter();
        active.spawn_worker(source_task(&gate, entered_tx)).unwrap()
    };
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let emission = active.emission_lease();
    assert!(emission.can_emit());
    let terminal = active.revoke().unwrap();
    assert!(!emission.can_emit());
    let retained = WorkerRetainedState::orphan_active(None, Some(reservation), None);
    let detached = DetachedCleanup::worker(worker.erase(), retained);

    let started = Instant::now();
    let outcome = runtime.block_on(terminal.coordinate(detached, &TOKIO_DEADLINES));
    assert_eq!(outcome, BridgeCleanupOutcome::Quarantined);
    assert!(started.elapsed() >= Duration::from_millis(4_500));
    let blocked = owner.begin_http_subscription().unwrap();
    assert!(blocked.activate(BridgeOperationKind::Subscription).is_err());
    assert!(matches!(
        bytes.try_reserve(1),
        Err(ByteReservationError::Capacity)
    ));
    assert_eq!(poll_cleanup(&cleanup), Poll::Pending);

    gate.release();
    runtime.block_on(std::future::poll_fn(|context| {
        match cleanup.poll_one(context) {
            Poll::Ready(Ok(true)) => Poll::Ready(()),
            Poll::Ready(Ok(false)) | Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => panic!("cleanup failed: {error:?}"),
        }
    }));
    let active = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let replacement = bytes.try_reserve(EVENT_BYTES).unwrap();
    assert!(matches!(
        bytes.try_reserve(1),
        Err(ByteReservationError::Capacity)
    ));
    assert!(!owner.delivery().snapshot().corrupted);
    drop(replacement);
    drop(active);
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}

#[test]
fn last_registry_drop_transfers_the_join_to_the_process_reaper() {
    let runtime = runtime();
    let gate = BlockingGate::new();
    let owner = owner();
    let guard = owner.register_worker_runtime().unwrap();
    let cleanup = owner.cleanup_driver();
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
    active
        .revoke()
        .unwrap()
        .quarantine(DetachedCleanup::worker(
            worker.erase(),
            WorkerRetainedState::empty(),
        ))
        .unwrap();
    let mut shutdown = guard.shutdown().unwrap();
    assert_eq!(
        Pin::new(&mut shutdown).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    );
    drop(cleanup);
    drop(owner);
    let shutdown = shutdown.into_registration_shutdown();

    gate.release();
    runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(2), shutdown).await })
        .expect("process reaper did not collect the worker")
        .unwrap();
}
