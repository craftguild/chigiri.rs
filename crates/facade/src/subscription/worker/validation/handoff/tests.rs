//! Unaccepted observation stays in the original task through payload disposal.

use std::future::Future;
use std::pin::Pin;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::task::{Context, Poll};
use std::time::Duration;

use super::*;
use crate::subscription::worker::managed_task::ManagedSubscriptionTask;
use crate::transport::websocket::AuthorizationErrorObserver;

#[derive(Default)]
struct Probe {
    gate: Mutex<(bool, bool)>,
    changed: Condvar,
    owner: Mutex<Option<tokio::task::Id>>,
    observed_owner: AtomicBool,
    reported: AtomicBool,
    calls: AtomicUsize,
}

struct Observe(Arc<Probe>);
impl AuthorizationErrorObserver for Observe {
    fn observe(&self, _: &(dyn std::error::Error + Send + Sync + 'static)) {
        if self.0.reported.swap(true, Ordering::SeqCst) {
            return;
        }
        self.0.observed_owner.store(
            *self.0.owner.lock().unwrap() == tokio::task::try_id(),
            Ordering::SeqCst,
        );
        self.0.calls.fetch_add(1, Ordering::SeqCst);
    }
}

struct Work {
    _sender: Sender<()>,
    probe: Arc<Probe>,
}
impl Future for Work {
    type Output = Result<(), SubscriptionWorkerRuntimeError>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        *self.probe.owner.lock().unwrap() = Some(tokio::task::id());
        std::panic::panic_any(Payload(self.probe.clone()));
    }
}
struct Payload(Arc<Probe>);
impl Drop for Payload {
    fn drop(&mut self) {
        let mut state = self.0.gate.lock().unwrap();
        state.0 = true;
        self.0.changed.notify_all();
        while !state.1 {
            state = self.0.changed.wait(state).unwrap();
        }
    }
}

#[test]
fn closing_an_unaccepted_receiver_cannot_claim_observation_during_task_payload_disposal() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .unwrap();
    let probe = Arc::new(Probe::default());
    let control = crate::managed::Control::new();
    let activity = EventActivity::new(Some(Arc::downgrade(&control)));
    let reporter = HttpFaultReporter::authorization(Arc::new(Observe(probe.clone())));
    let (sender, mut receiver) = channel(activity.clone(), Some(reporter.clone()));
    let mut task = runtime.spawn(ManagedSubscriptionTask::new(
        Work {
            _sender: sender,
            probe: probe.clone(),
        },
        activity.clone(),
        reporter,
    ));
    // Join the same owned native task even if a parent assertion fails. No
    // fixture failure may detach it or leave its destructor gate closed.
    let assertions = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let state = probe.gate.lock().unwrap();
        let (state, timeout) = probe
            .changed
            .wait_timeout_while(state, Duration::from_secs(2), |state| !state.0)
            .unwrap();
        let entered = state.0 && !timeout.timed_out();
        drop(state);
        assert!(entered, "the original task must enter payload destruction");
        assert!(activity.task_failed());
        receiver.close();
        assert_eq!(
            probe.calls.load(Ordering::SeqCst),
            0,
            "unaccepted parent closure cannot steal private observation"
        );
    }));
    probe
        .gate
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .1 = true;
    probe.changed.notify_all();
    runtime.block_on(async {
        let joined = tokio::time::timeout(Duration::from_secs(2), &mut task).await;
        match joined {
            Ok(result) => assert_eq!(
                result.unwrap(),
                Err(SubscriptionWorkerRuntimeError::Invariant)
            ),
            Err(_) => {
                task.abort();
                let _ = task.await;
                panic!("released guarded task must join");
            }
        }
    });
    if let Err(payload) = assertions {
        std::panic::resume_unwind(payload);
    }
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    assert!(probe.observed_owner.load(Ordering::SeqCst));
}
