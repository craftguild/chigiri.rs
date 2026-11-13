//! Resource-free event marker and weak endpoint cancellation authority.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use super::Control;

#[derive(Clone)]
pub(crate) struct EventActivity {
    state: Arc<ActivityState>,
    control: Option<Weak<Control>>,
}

struct ActivityState {
    active: AtomicBool,
    setup_cancelled: AtomicBool,
    faulted: AtomicBool,
    poisoned: AtomicBool,
}

impl EventActivity {
    pub(crate) fn new(control: Option<Weak<Control>>) -> Self {
        Self {
            state: Arc::new(ActivityState {
                active: AtomicBool::new(false),
                setup_cancelled: AtomicBool::new(false),
                faulted: AtomicBool::new(false),
                poisoned: AtomicBool::new(false),
            }),
            control,
        }
    }

    pub(crate) fn control(&self) -> Option<Arc<Control>> {
        self.control.as_ref().and_then(Weak::upgrade)
    }

    pub(crate) fn is_managed(&self) -> bool {
        self.control.is_some()
    }

    pub(crate) fn is_stopping(&self) -> bool {
        self.is_managed() && self.control().is_none_or(|control| control.is_stopping())
    }

    /// Only the closed managed task sets this marker and owns its observation.
    pub(crate) fn fail_task(&self) {
        self.state.faulted.store(true, Ordering::Release);
        if let Some(control) = self.control() {
            control.fail();
        }
    }

    pub(crate) fn poison_task(&self) {
        self.state.poisoned.store(true, Ordering::Release);
        self.fail_task();
    }

    pub(crate) fn task_failed(&self) -> bool {
        self.state.faulted.load(Ordering::Acquire)
    }

    pub(crate) fn task_poisoned(&self) -> bool {
        self.state.poisoned.load(Ordering::Acquire)
    }

    pub(crate) fn is_active(&self) -> bool {
        self.state.active.load(Ordering::Acquire)
    }

    /// Closed task-side cancellation evidence survives a full observation port.
    pub(crate) fn cancel_setup(&self) {
        self.state.setup_cancelled.store(true, Ordering::Release);
    }

    pub(crate) fn setup_cancelled(&self) -> bool {
        self.state.setup_cancelled.load(Ordering::Acquire)
    }

    pub(crate) fn finish(&self) {
        self.state.active.store(false, Ordering::Release);
    }

    pub(crate) fn start(&self) -> bool {
        let control = self.control();
        let gate = match &control {
            Some(control) => match control.registration_gate() {
                Ok(gate) => Some(gate),
                Err(_) => return false,
            },
            None if self.control.is_some() => return false,
            None => None,
        };
        self.state.active.store(true, Ordering::Release);
        drop(gate);
        true
    }

    pub(crate) async fn source_stopped(&self) {
        match self.control() {
            Some(control) => control.source_stopped().await,
            None if self.control.is_some() => {}
            None => std::future::pending().await,
        }
    }
}
