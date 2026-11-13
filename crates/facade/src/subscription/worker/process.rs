//! One process-owned reaper for concrete Tokio subscription workers.

mod reaping;
mod shutdown;
pub(crate) mod supervisors;

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::task::{Wake, Waker};
use std::thread::{self, JoinHandle};

use super::error::SubscriptionWorkerRuntimeError;
use super::registration::{EndpointWorkerOwner, WorkerRegistration};
pub use shutdown::SubscriptionProcessShutdown;

static PROCESS_REAPER: OnceLock<Result<Arc<ProcessReaper>, ()>> = OnceLock::new();

#[cfg(test)]
pub(crate) fn poisoned_for_test() -> bool {
    PROCESS_REAPER
        .get()
        .and_then(|process| process.as_ref().ok())
        .is_some_and(|process| lock_process(&process.shared).poisoned)
}

/// Failure-only observation: never poll, abort, notify or recover a poisoned lock.
/// Separate snapshots are deliberately not an atomic view of concurrent progress.
#[cfg(test)]
pub(crate) fn diagnostic_for_test() -> String {
    let Some(Ok(process)) = PROCESS_REAPER.get() else {
        return "process reaper unavailable".to_owned();
    };
    let (status, epoch, poisoned, registrations, supervisors) = {
        let state = match process.shared.state.try_lock() {
            Ok(state) => state,
            Err(error) => return format!("process state: {error}"),
        };
        let status = match state.status {
            ProcessStatus::Running => "Running",
            ProcessStatus::Draining => "Draining",
            ProcessStatus::Stopped => "Stopped",
            ProcessStatus::Joined => "Joined",
        };
        (
            status,
            state.epoch,
            state.poisoned,
            state
                .registrations
                .iter()
                .map(|(id, record)| (*id, Arc::clone(record)))
                .collect::<Vec<_>>(),
            state
                .supervisors
                .iter()
                .map(|(id, record)| (*id, Arc::clone(record)))
                .collect::<Vec<_>>(),
        )
    };
    let thread = match process.thread.try_lock() {
        Ok(thread) => format!("{:?}", thread.as_ref().map(JoinHandle::is_finished)),
        Err(error) => error.to_string(),
    };
    let registrations = registrations
        .iter()
        .map(|(id, record)| (*id, record.diagnostic_for_test()))
        .collect::<Vec<_>>();
    let supervisors = supervisors
        .iter()
        .map(|(id, record)| (*id, record.diagnostic_for_test()))
        .collect::<Vec<_>>();
    format!(
        "status={status}, epoch={epoch}, poisoned={poisoned}, reaper_finished={thread}, registrations={registrations:?}, supervisors={supervisors:?}"
    )
}

pub(super) struct ProcessShared {
    state: Mutex<ProcessState>,
    signal: Condvar,
    wake: OnceLock<Arc<ProcessWake>>,
}

pub(super) struct ProcessState {
    status: ProcessStatus,
    shutdown_owner: bool,
    next_registration: u64,
    registrations: BTreeMap<u64, Arc<WorkerRegistration>>,
    supervisors: BTreeMap<u64, Arc<supervisors::SupervisorCompletion>>,
    cursor: Option<u64>,
    supervisor_cursor: Option<u64>,
    epoch: u64,
    shutdown_waker: Option<Waker>,
    poisoned: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum ProcessStatus {
    Running,
    Draining,
    Stopped,
    Joined,
}

pub(super) struct ProcessReaper {
    pub(super) shared: Arc<ProcessShared>,
    pub(super) thread: Mutex<Option<JoinHandle<()>>>,
}

struct ProcessWake {
    shared: Weak<ProcessShared>,
}

impl Wake for ProcessWake {
    fn wake(self: Arc<Self>) {
        self.notify();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.notify();
    }
}

impl ProcessWake {
    fn notify(&self) {
        if let Some(shared) = self.shared.upgrade() {
            notify_shared(&shared);
        }
    }
}

pub(crate) fn register_endpoint() -> Result<EndpointWorkerOwner, SubscriptionWorkerRuntimeError> {
    let process = process_reaper()?;
    let mut state = process
        .shared
        .state
        .lock()
        .map_err(|_| SubscriptionWorkerRuntimeError::Invariant)?;
    if state.status != ProcessStatus::Running {
        return Err(SubscriptionWorkerRuntimeError::Shutdown);
    }
    let id = state
        .next_registration
        .checked_add(1)
        .ok_or(SubscriptionWorkerRuntimeError::Invariant)?;
    state.next_registration = id;
    let registration = WorkerRegistration::new(id, Arc::downgrade(&process.shared));
    state.registrations.insert(id, Arc::clone(&registration));
    state.epoch = state.epoch.wrapping_add(1);
    drop(state);
    process.shared.signal.notify_one();
    Ok(EndpointWorkerOwner::new(registration))
}

pub(crate) fn shutdown_process_runtime()
-> Result<SubscriptionProcessShutdown, SubscriptionWorkerRuntimeError> {
    let process = process_reaper()?;
    let (registrations, supervisors) = {
        let mut state = process
            .shared
            .state
            .lock()
            .map_err(|_| SubscriptionWorkerRuntimeError::Invariant)?;
        match state.status {
            ProcessStatus::Running => {
                state.status = ProcessStatus::Draining;
            }
            ProcessStatus::Draining | ProcessStatus::Stopped if !state.shutdown_owner => {}
            ProcessStatus::Draining | ProcessStatus::Stopped | ProcessStatus::Joined => {
                return Err(SubscriptionWorkerRuntimeError::Shutdown);
            }
        }
        state.shutdown_owner = true;
        state.epoch = state.epoch.wrapping_add(1);
        (
            state.registrations.values().cloned().collect::<Vec<_>>(),
            state.supervisors.values().cloned().collect::<Vec<_>>(),
        )
    };
    for registration in registrations {
        registration.close();
    }
    for supervisor in supervisors {
        supervisor.stop();
    }
    process.shared.signal.notify_one();
    Ok(SubscriptionProcessShutdown::new(process))
}

pub(super) fn process_accepting(shared: &ProcessShared) -> bool {
    shared
        .state
        .lock()
        .map(|state| state.status == ProcessStatus::Running)
        .unwrap_or(false)
}

pub(super) fn notify_process(process: &Weak<ProcessShared>) {
    if let Some(shared) = process.upgrade() {
        notify_shared(&shared);
    }
}

pub(super) fn mark_process_poisoned(process: &Weak<ProcessShared>) {
    let Some(shared) = process.upgrade() else {
        return;
    };
    let mut state = lock_process(&shared);
    state.poisoned = true;
    state.epoch = state.epoch.wrapping_add(1);
    let waker = state.shutdown_waker.take();
    drop(state);
    if let Some(waker) = waker {
        waker.wake();
    }
    shared.signal.notify_one();
}

pub(super) fn unregister_if_drained(registration: &WorkerRegistration) {
    if !registration.drained() {
        return;
    }
    let Some(shared) = registration.process.upgrade() else {
        return;
    };
    let mut state = lock_process(&shared);
    if state
        .registrations
        .get(&registration.id())
        .is_some_and(|current| std::ptr::eq(current.as_ref(), registration))
    {
        state.registrations.remove(&registration.id());
        state.epoch = state.epoch.wrapping_add(1);
    }
    let waker = state.shutdown_waker.take();
    drop(state);
    if let Some(waker) = waker {
        waker.wake();
    }
    shared.signal.notify_one();
}

fn process_reaper() -> Result<Arc<ProcessReaper>, SubscriptionWorkerRuntimeError> {
    PROCESS_REAPER
        .get_or_init(|| ProcessReaper::start().map_err(|_| ()))
        .as_ref()
        .map(Arc::clone)
        .map_err(|_| SubscriptionWorkerRuntimeError::Startup)
}

impl ProcessReaper {
    fn start() -> std::io::Result<Arc<Self>> {
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
        let wake = Arc::new(ProcessWake {
            shared: Arc::downgrade(&shared),
        });
        shared
            .wake
            .set(wake)
            .map_err(|_| std::io::Error::other("subscription process waker already initialized"))?;
        let thread_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("chigiri-subscription-reaper".to_owned())
            .spawn(move || reaping::run_reaper(thread_shared))?;
        #[cfg(test)]
        REAPER_THREADS_STARTED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Arc::new(Self {
            shared,
            thread: Mutex::new(Some(thread)),
        }))
    }
}

fn notify_shared(shared: &ProcessShared) {
    let mut state = lock_process(shared);
    state.epoch = state.epoch.wrapping_add(1);
    drop(state);
    shared.signal.notify_one();
}

pub(super) fn lock_process(shared: &ProcessShared) -> std::sync::MutexGuard<'_, ProcessState> {
    match shared.state.lock() {
        Ok(state) => state,
        Err(poisoned) => {
            let mut state = poisoned.into_inner();
            state.poisoned = true;
            state
        }
    }
}

#[cfg(test)]
static REAPER_THREADS_STARTED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
static REAPER_THREADS_EXITED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
static REAPER_THREADS_JOINED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
pub(super) fn record_reaper_join() {
    REAPER_THREADS_JOINED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
pub(super) fn reaper_thread_counts() -> (usize, usize, usize) {
    use std::sync::atomic::Ordering;

    (
        REAPER_THREADS_STARTED.load(Ordering::SeqCst),
        REAPER_THREADS_EXITED.load(Ordering::SeqCst),
        REAPER_THREADS_JOINED.load(Ordering::SeqCst),
    )
}
