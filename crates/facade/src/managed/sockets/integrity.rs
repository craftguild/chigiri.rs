//! Resource-free original connection ownership shared with SDK join recovery.

use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};

use crate::managed::{Control, EventActivity};
use crate::subscription::bridge_resources::{
    TerminalCleanupOwner, WebSocketConnectionPhysicalLease,
};

pub(crate) struct SocketScope {
    activity: EventActivity,
    report: AtomicBool,
    finished: AtomicBool,
    retained: Mutex<Retained>,
}

struct Retained {
    physical: Option<WebSocketConnectionPhysicalLease>,
    cleanup: Option<TerminalCleanupOwner>,
}

impl SocketScope {
    #[cfg(test)]
    pub(super) fn retains_ticket_for_test(&self) -> bool {
        self.retained.lock().unwrap().cleanup.is_some()
    }

    pub(super) fn new(
        control: Weak<Control>,
        physical: WebSocketConnectionPhysicalLease,
    ) -> Arc<Self> {
        Arc::new(Self {
            activity: EventActivity::new(Some(control)),
            report: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            retained: Mutex::new(Retained {
                physical: Some(physical),
                cleanup: None,
            }),
        })
    }

    pub(crate) fn activity(&self) -> EventActivity {
        self.activity.clone()
    }
    pub(super) fn failed(&self) -> bool {
        self.report.load(Ordering::Acquire)
    }
    pub(super) fn poisoned(&self) -> bool {
        self.activity.task_poisoned()
    }
    pub(super) fn fail(&self) {
        self.report.store(true, Ordering::Release);
        self.activity.fail_task();
    }
    pub(super) fn poison(&self) {
        self.report.store(true, Ordering::Release);
        self.activity.poison_task();
    }

    pub(crate) fn retain_cleanup(&self, owner: TerminalCleanupOwner) {
        let mut proposed = Some(owner);
        let mut duplicate = false;
        let mut retained = self
            .retained
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !self.finished.load(Ordering::Acquire) || self.poisoned() {
            if retained.cleanup.is_none() {
                retained.cleanup = proposed.take();
            } else {
                duplicate = true;
            }
        }
        drop(retained);
        if duplicate {
            self.poison();
        }
        drop(proposed);
    }

    pub(super) fn finish(&self) {
        self.finished.store(true, Ordering::Release);
        if self.poisoned() {
            return;
        }
        let (cleanup, physical) = {
            let mut retained = self
                .retained
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            (retained.cleanup.take(), retained.physical.take())
        };
        drop(cleanup);
        drop(physical);
    }
}
