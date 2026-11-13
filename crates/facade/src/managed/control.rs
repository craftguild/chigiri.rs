//! Ordered admission and completion counts; notification carries no user data.

#[cfg(test)]
mod tests;
mod transition;

use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::watch;
use tokio::time::Instant;

use super::ManagedServiceError;
use crate::ShutdownOutcome;

pub(crate) struct Control {
    state: Mutex<State>,
    changed: watch::Sender<()>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StopPhase {
    Running,
    Graceful { deadline: Instant },
    Cancelled,
}

struct State {
    phase: StopPhase,
    requests: usize,
    activated: bool,
    outcome: Option<ShutdownOutcome>,
}

pub(crate) struct AdmissionGate<'a> {
    _state: MutexGuard<'a, State>,
}

impl AdmissionGate<'_> {
    pub(crate) fn phase(&self) -> StopPhase {
        self._state.phase
    }
}

impl Control {
    pub(crate) fn registration_gate(&self) -> Result<AdmissionGate<'_>, ManagedServiceError> {
        let state = self.lock();
        if state.phase != StopPhase::Running {
            return Err(ManagedServiceError::Closed);
        }
        Ok(AdmissionGate { _state: state })
    }

    /// Linearizes a bounded framework emission/encoding transition. Never hold
    /// this gate while polling application work, a native socket, or a waker.
    pub(crate) fn emission_gate(&self) -> Result<AdmissionGate<'_>, ManagedServiceError> {
        let mut state = self.lock();
        if matches!(state.phase, StopPhase::Graceful { deadline } if Instant::now() >= deadline) {
            state.phase = StopPhase::Cancelled;
            state.outcome.get_or_insert(ShutdownOutcome::Expired);
        }
        // The hook and supervisor own the original deadline wake. This gate
        // never invokes notification/waker code inside a transport timeline.
        if state.phase == StopPhase::Cancelled {
            return Err(ManagedServiceError::Closed);
        }
        Ok(AdmissionGate { _state: state })
    }

    pub(crate) fn activation_started(&self) -> Result<(), ManagedServiceError> {
        let mut state = self.lock();
        if state.phase != StopPhase::Running {
            return Err(ManagedServiceError::Closed);
        }
        state.activated = true;
        Ok(())
    }

    pub(crate) fn new() -> Arc<Self> {
        let (changed, _) = watch::channel(());
        Arc::new(Self {
            state: Mutex::new(State {
                phase: StopPhase::Running,
                requests: 0,
                activated: false,
                outcome: None,
            }),
            changed,
        })
    }

    pub(crate) fn admit(self: &Arc<Self>) -> Result<RequestPermit, ManagedServiceError> {
        let mut state = self.lock();
        if state.phase != StopPhase::Running {
            return Err(ManagedServiceError::Closed);
        }
        let Some(count) = state.requests.checked_add(1) else {
            drop(state);
            self.fail();
            return Err(ManagedServiceError::Capacity);
        };
        state.requests = count;
        Ok(RequestPermit {
            control: Arc::clone(self),
        })
    }

    pub(crate) fn fail(&self) {
        let mut state = self.lock();
        state.phase = StopPhase::Cancelled;
        state
            .outcome
            .get_or_insert(ShutdownOutcome::InfrastructureFailure);
        drop(state);
        self.notify();
    }

    pub(crate) fn phase(&self) -> StopPhase {
        self.lock().phase
    }

    pub(crate) fn is_stopping(&self) -> bool {
        self.phase() != StopPhase::Running
    }

    pub(crate) fn requests_idle(&self) -> bool {
        self.lock().requests == 0
    }

    pub(crate) fn outcome(&self) -> Option<ShutdownOutcome> {
        self.lock().outcome
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<()> {
        self.changed.subscribe()
    }

    pub(crate) fn notify(&self) {
        self.changed.send_replace(());
    }

    pub(crate) async fn cancelled(&self) {
        let mut changed = self.subscribe();
        while self.phase() != StopPhase::Cancelled {
            if changed.changed().await.is_err() {
                self.fail();
                break;
            }
        }
    }

    pub(crate) async fn source_stopped(&self) {
        let mut changed = self.subscribe();
        while !self.is_stopping() {
            if changed.changed().await.is_err() {
                self.fail();
                break;
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.phase = StopPhase::Cancelled;
                state
                    .outcome
                    .get_or_insert(ShutdownOutcome::InfrastructureFailure);
                state
            }
        }
    }
}

pub(crate) struct RequestPermit {
    control: Arc<Control>,
}

impl Drop for RequestPermit {
    fn drop(&mut self) {
        let mut state = self.control.lock();
        match state.requests.checked_sub(1) {
            Some(count) => state.requests = count,
            None => {
                state.phase = StopPhase::Cancelled;
                state
                    .outcome
                    .get_or_insert(ShutdownOutcome::InfrastructureFailure);
            }
        }
        drop(state);
        self.control.notify();
    }
}
