//! Local process state with owned native joins and no background reaper thread.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::time::Duration;

use super::super::*;
use crate::subscription::worker::{ConcreteWorker, RetiredWorkerOwner, WorkerRetainedState};

pub(super) struct Fixture {
    pub(super) shared: Arc<ProcessShared>,
    runtime: tokio::runtime::Runtime,
    registrations: Vec<Arc<WorkerRegistration>>,
    aborts: Vec<tokio::task::AbortHandle>,
}

impl Fixture {
    fn new() -> Self {
        let shared = Arc::new(ProcessShared {
            state: Mutex::new(ProcessState {
                status: ProcessStatus::Running,
                shutdown_owner: false,
                next_registration: 0,
                registrations: BTreeMap::new(),
                supervisors: BTreeMap::new(),
                cursor: None,
                supervisor_cursor: None,
                epoch: 0,
                shutdown_waker: None,
                poisoned: false,
            }),
            signal: Condvar::new(),
            wake: OnceLock::new(),
        });
        assert!(
            shared
                .wake
                .set(Arc::new(ProcessWake {
                    shared: Arc::downgrade(&shared),
                }))
                .is_ok()
        );
        Self {
            shared,
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap(),
            registrations: Vec::new(),
            aborts: Vec::new(),
        }
    }

    pub(super) fn registration(&mut self) -> Arc<WorkerRegistration> {
        let mut state = lock_process(&self.shared);
        state.next_registration += 1;
        let id = state.next_registration;
        let registration = WorkerRegistration::new(id, Arc::downgrade(&self.shared));
        state.registrations.insert(id, registration.clone());
        self.registrations.push(registration.clone());
        registration
    }

    pub(super) fn ready(&mut self, registration: &Arc<WorkerRegistration>) {
        self.retire(registration, async {}, true);
    }

    pub(super) fn pending(&mut self, registration: &Arc<WorkerRegistration>) {
        self.retire(registration, std::future::pending(), false);
    }

    fn retire(
        &mut self,
        registration: &Arc<WorkerRegistration>,
        work: impl Future<Output = ()> + Send + 'static,
        ready: bool,
    ) {
        let generation = registration.reserve_generation().unwrap();
        let slot = generation.start().unwrap();
        let handle = self.runtime.spawn(work);
        self.aborts.push(handle.abort_handle());
        if ready {
            let finished = self.runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(2), async {
                    while !handle.is_finished() {
                        tokio::task::yield_now().await;
                    }
                })
                .await
            });
            if finished.is_err() {
                handle.abort();
                let _ = self.runtime.block_on(handle);
                panic!("the owned native fixture task did not reach JoinReady");
            }
        }
        let worker = ConcreteWorker::new(handle, slot);
        generation.transfer();
        worker
            .erase()
            .retire(RetiredWorkerOwner::orphan(WorkerRetainedState::empty()));
    }

    fn finish(&mut self) {
        for abort in &self.aborts {
            abort.abort();
        }
        for registration in &self.registrations {
            registration.close();
        }
        self.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                while !lock_process(&self.shared).registrations.is_empty() {
                    poll_registrations(&self.shared);
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("fixture teardown owns and joins every retired native task");
        });
    }
}

pub(super) fn with_fixture(test: impl FnOnce(&mut Fixture)) {
    let mut fixture = Fixture::new();
    let result = catch_unwind(AssertUnwindSafe(|| test(&mut fixture)));
    fixture.finish();
    if let Err(payload) = result {
        resume_unwind(payload);
    }
}
