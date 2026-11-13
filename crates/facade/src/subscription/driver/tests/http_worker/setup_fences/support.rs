//! Owned phase barrier and native task observation for paused HTTP preparation.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::task::{Context, Waker};
use std::time::Duration;

use chigiri_runtime::{
    ErrorEvent, ErrorObserver, LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult,
};

use super::super::super::harness;
use super::super::super::state::{AppContext, State, count};
use super::super::support::{pending, run, runtime};
use crate::managed::Control;
use crate::subscription::worker::SubscriptionRuntimeGuard;
use crate::transport::EndpointState;
use crate::transport::streaming::{
    HttpSubscriptionPreparation, InitialCommitTestFault, InitialWorkerProbe, InitialWorkerStage,
};

struct Release(Option<SyncSender<()>>);
impl Release {
    fn send(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.send();
    }
}

pub(super) struct Boundary {
    phase: Option<LifecyclePhase>,
    pub(super) reached: AtomicUsize,
    pub(super) errors: AtomicUsize,
    release: Mutex<Receiver<()>>,
    events: Mutex<Vec<(LifecyclePhase, LifecycleResult)>>,
}

#[derive(Clone)]
struct Observe(Arc<Boundary>);
impl LifecycleObserver for Observe {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.0
            .events
            .lock()
            .unwrap()
            .push((event.phase(), event.result()));
        if self.0.phase == Some(event.phase()) && event.result() == LifecycleResult::Succeeded {
            assert_eq!(self.0.reached.fetch_add(1, Ordering::SeqCst), 0);
            self.0
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(2))
                .expect("owned phase barrier release");
        }
    }
}
impl ErrorObserver for Observe {
    fn observe_error(&self, _: ErrorEvent<'_>) {
        self.0.errors.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) struct Fixture {
    release: Release,
    pub(super) runtime: tokio::runtime::Runtime,
    pub(super) endpoint: Arc<EndpointState<AppContext>>,
    pub(super) control: Arc<Control>,
    pub(super) boundary: Arc<Boundary>,
    tasks: InitialWorkerProbe,
    guard: Option<SubscriptionRuntimeGuard>,
}

impl Fixture {
    pub(super) fn replace_factory(
        &mut self,
        factory: impl chigiri_http::ContextFactory<AppContext, super::super::super::state::HookState>,
    ) {
        let endpoint = Arc::get_mut(&mut self.endpoint).unwrap();
        endpoint.context_factory = crate::transport::erase_factory(factory);
        Arc::get_mut(&mut endpoint.subscription_execution)
            .unwrap()
            .context_factory = endpoint.context_factory.clone();
    }

    pub(super) fn new(probe: Arc<State>, phase: Option<LifecyclePhase>) -> Self {
        let runtime = runtime();
        let (release, released) = sync_channel(1);
        let boundary = Arc::new(Boundary {
            phase,
            reached: AtomicUsize::new(0),
            errors: AtomicUsize::new(0),
            release: Mutex::new(released),
            events: Mutex::new(Vec::new()),
        });
        let mut endpoint = harness::endpoint(probe, 16 * 1024);
        let state = Arc::get_mut(&mut endpoint).unwrap();
        state.schema = state
            .schema
            .reconfigure()
            .lifecycle_observer(Observe(boundary.clone()))
            .error_observer(Observe(boundary.clone()))
            .build();
        let execution = Arc::get_mut(&mut state.subscription_execution).unwrap();
        execution.schema = state.schema.clone();
        let control = Control::new();
        control.activation_started().unwrap();
        execution.managed.set(Arc::downgrade(&control)).unwrap();
        let guard = state
            .subscriptions
            .resources
            .register_worker_runtime()
            .unwrap();
        Self {
            release: Release(Some(release)),
            runtime,
            endpoint,
            control,
            boundary,
            tasks: InitialWorkerProbe::default(),
            guard: Some(guard),
        }
    }

    pub(super) fn preparation(
        &self,
        profile: chigiri_http::SubscriptionTransport,
        body: &[u8],
    ) -> HttpSubscriptionPreparation {
        run(
            &self.runtime,
            pending(self.endpoint.clone(), profile, body, false),
        )
        .with_commit_fault(InitialCommitTestFault::WorkerProbe(self.tasks.clone()))
        .prepare()
    }

    pub(super) fn reach<F: Future>(&self, mut future: Pin<&mut F>, done: impl Fn() -> bool) {
        self.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(1), async {
                while !done() {
                    assert!(
                        future
                            .as_mut()
                            .poll(&mut Context::from_waker(Waker::noop()))
                            .is_pending()
                    );
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "phase barrier {:?}: {:?}",
                    self.boundary.phase,
                    self.boundary.events.lock().unwrap()
                )
            });
        });
    }

    pub(super) fn release(&mut self) {
        self.release.send();
    }

    pub(super) fn finished(&self) {
        self.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(1), async {
                while !self.tasks.finished(InitialWorkerStage::Setup) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("original native setup task must finish without preparation polling");
        });
    }

    pub(super) fn task_finished(&self) -> bool {
        self.tasks.finished(InitialWorkerStage::Setup)
    }

    pub(super) fn finish(mut self) {
        self.release();
        run(
            &self.runtime,
            self.guard.take().unwrap().shutdown().unwrap(),
        )
        .unwrap();
        assert_eq!(
            self.endpoint
                .subscriptions
                .resources
                .delivery()
                .byte_owner()
                .used(),
            0
        );
        let snapshot = self.endpoint.subscriptions.resources.delivery().snapshot();
        assert_eq!(snapshot.endpoint_active_subscriptions, 0);
        assert_eq!(snapshot.endpoint_pending, 0);
        assert_eq!(self.endpoint.in_flight.load(Ordering::Acquire), 0);
        assert!(
            self.endpoint
                .subscriptions
                .resources
                .begin_http_subscription()
                .is_ok()
        );
    }

    pub(super) fn cancelled_requests(&self) -> usize {
        self.boundary
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(phase, result)| {
                *phase == LifecyclePhase::RequestCompleted && *result == LifecycleResult::Cancelled
            })
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.release.send();
    }
}

pub(super) fn reached(boundary: &Boundary) -> bool {
    count(&boundary.reached) == 1
}
