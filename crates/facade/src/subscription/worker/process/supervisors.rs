//! Process-owned joins for the one closed managed supervisor per activation.
//!
//! This table is parallel to operation registrations. Keeping joins inside an
//! operation registration would create a cycle through the driver's permits.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use tokio::task::JoinHandle;

use super::{ProcessShared, ProcessStatus, lock_process, notify_shared, process_reaper};
use crate::managed::{Control, ManagedServiceError, Supervisor};
use crate::subscription::worker::completion::dispose_payload;

pub(crate) struct SupervisorCompletion {
    control: Arc<Control>,
    sockets: Arc<crate::managed::SocketRegistry>,
    join: Mutex<JoinState>,
}

enum JoinState {
    Starting,
    Running(JoinHandle<()>),
    Polling,
    Completed,
    Poisoned { _join: JoinHandle<()> },
}

impl SupervisorCompletion {
    #[cfg(test)]
    pub(super) fn diagnostic_for_test(&self) -> String {
        let state = match self.join.try_lock() {
            Ok(state) => state,
            Err(error) => return format!("{error}"),
        };
        match &*state {
            JoinState::Starting => "Starting".to_owned(),
            JoinState::Running(join) => format!("Running finished={}", join.is_finished()),
            JoinState::Polling => "Polling".to_owned(),
            JoinState::Completed => "Completed".to_owned(),
            JoinState::Poisoned { _join } => format!("Poisoned finished={}", _join.is_finished()),
        }
    }

    pub(super) fn stop(&self) {
        self.control.stop();
    }

    fn poll(&self, context: &mut Context<'_>) -> (bool, bool) {
        let (mut state, corrupted) = match self.join.lock() {
            Ok(state) => (state, false),
            Err(poisoned) => (poisoned.into_inner(), true),
        };
        if !matches!(*state, JoinState::Running(_)) {
            let completed = matches!(*state, JoinState::Completed);
            drop(state);
            if corrupted {
                self.control.fail();
            }
            return (completed, false);
        }
        let JoinState::Running(mut join) = std::mem::replace(&mut *state, JoinState::Polling)
        else {
            unreachable!("matched supervisor join");
        };
        drop(state);
        if corrupted {
            self.control.fail();
        }
        // Neither polling nor panic-payload destruction happens under a lock.
        let complete = match Pin::new(&mut join).poll(context) {
            Poll::Pending => false,
            Poll::Ready(Ok(())) => true,
            Poll::Ready(Err(error)) => {
                self.control.fail();
                if error.is_panic() {
                    let _ = dispose_payload(error.into_panic());
                }
                true
            }
        };
        let poisoned = complete && self.sockets.has_poisoned();
        *self
            .join
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = if complete {
            if poisoned {
                JoinState::Poisoned { _join: join }
            } else {
                JoinState::Completed
            }
        } else {
            JoinState::Running(join)
        };
        (complete && !poisoned, poisoned)
    }
}

pub(crate) fn spawn_managed_supervisor(supervisor: Supervisor) -> Result<(), ManagedServiceError> {
    let process = process_reaper().map_err(|_| ManagedServiceError::Startup)?;
    let runtime =
        tokio::runtime::Handle::try_current().map_err(|_| ManagedServiceError::MissingRuntime)?;
    let control = supervisor.control();
    let sockets = supervisor.sockets();
    let record = Arc::new(SupervisorCompletion {
        control: Arc::clone(&control),
        sockets,
        join: Mutex::new(JoinState::Starting),
    });
    let id = {
        let mut state = lock_process(&process.shared);
        if state.status != ProcessStatus::Running || state.poisoned {
            return Err(ManagedServiceError::Startup);
        }
        let id = state
            .next_registration
            .checked_add(1)
            .ok_or(ManagedServiceError::Capacity)?;
        state.next_registration = id;
        state.supervisors.insert(id, Arc::clone(&record));
        id
    };
    match catch_unwind(AssertUnwindSafe(|| runtime.spawn(supervisor))) {
        Ok(join) => {
            let stopped = join.is_finished();
            *record
                .join
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = JoinState::Running(join);
            notify_shared(&process.shared);
            if stopped || control.is_stopping() {
                control.fail();
                Err(ManagedServiceError::Startup)
            } else {
                Ok(())
            }
        }
        Err(payload) => {
            control.fail();
            let _ = dispose_payload(payload);
            let removed = lock_process(&process.shared).supervisors.remove(&id);
            drop(removed);
            notify_shared(&process.shared);
            Err(ManagedServiceError::Startup)
        }
    }
}

pub(super) fn poll_all(shared: &Arc<ProcessShared>) {
    let count = lock_process(shared).supervisors.len();
    let waker = Waker::from(Arc::clone(shared.wake.get().expect("process wake")));
    let mut context = Context::from_waker(&waker);
    for _ in 0..count {
        let selected = {
            let mut state = lock_process(shared);
            let next = state
                .supervisor_cursor
                .and_then(|cursor| {
                    state
                        .supervisors
                        .range((
                            std::ops::Bound::Excluded(cursor),
                            std::ops::Bound::Unbounded,
                        ))
                        .next()
                        .map(|(id, record)| (*id, Arc::clone(record)))
                })
                .or_else(|| {
                    state
                        .supervisors
                        .first_key_value()
                        .map(|(id, record)| (*id, Arc::clone(record)))
                });
            state.supervisor_cursor = next.as_ref().map(|(id, _)| *id);
            next
        };
        let Some((id, record)) = selected else {
            break;
        };
        let (complete, poisoned) = record.poll(&mut context);
        if poisoned {
            super::mark_process_poisoned(&Arc::downgrade(shared));
        }
        if complete {
            let removed = lock_process(shared).supervisors.remove(&id);
            drop(removed);
            notify_shared(shared);
        }
    }
}

#[cfg(test)]
pub(crate) fn retained_for_test(control: &Arc<Control>) -> bool {
    let process = process_reaper().unwrap();
    let state = lock_process(&process.shared);
    state
        .supervisors
        .values()
        .any(|record| Arc::ptr_eq(&record.control, control))
}

#[cfg(test)]
pub(crate) fn retained_socket_tickets_for_test(control: &Arc<Control>) -> usize {
    let process = process_reaper().unwrap();
    let record = lock_process(&process.shared)
        .supervisors
        .values()
        .find(|record| Arc::ptr_eq(&record.control, control))
        .cloned();
    record.map_or(0, |record| record.sockets.retained_tickets_for_test())
}

#[cfg(test)]
pub(crate) fn poisoned_for_test(control: &Arc<Control>) -> bool {
    let process = process_reaper().unwrap();
    let record = lock_process(&process.shared)
        .supervisors
        .values()
        .find(|record| Arc::ptr_eq(&record.control, control))
        .cloned();
    record.is_some_and(|record| matches!(*record.join.lock().unwrap(), JoinState::Poisoned { .. }))
}

#[cfg(test)]
pub(crate) fn abort_for_test(control: &Arc<Control>) -> bool {
    let process = process_reaper().unwrap();
    let record = lock_process(&process.shared)
        .supervisors
        .values()
        .find(|record| Arc::ptr_eq(&record.control, control))
        .cloned();
    let Some(record) = record else {
        return false;
    };
    let state = record.join.lock().unwrap();
    if let JoinState::Running(join) = &*state {
        join.abort();
        true
    } else {
        false
    }
}
