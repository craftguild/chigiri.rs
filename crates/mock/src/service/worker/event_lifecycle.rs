//! Order named callback starts against Graceful stop without cancelling current work.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crossbeam_channel::Sender;

#[derive(Default)]
pub(super) struct EventLifecycle {
    start: Mutex<()>,
    stopped: AtomicBool,
    running: AtomicBool,
    wake: OnceLock<Sender<()>>,
}

pub(super) struct Running(Arc<EventLifecycle>);

impl EventLifecycle {
    pub(super) fn install(&self, wake: Sender<()>) {
        assert!(
            self.wake.set(wake).is_ok(),
            "one event scheduler per worker"
        );
    }

    pub(super) fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    pub(super) fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub(super) fn stop(&self) {
        let start = self.start.lock().expect("event start fence");
        self.stopped.store(true, Ordering::Release);
        drop(start);
        if let Some(wake) = self.wake.get() {
            let _ = wake.try_send(());
        }
    }

    pub(super) fn start(&self) -> Option<MutexGuard<'_, ()>> {
        let start = self.start.lock().expect("event start fence");
        (!self.stopped()).then_some(start)
    }

    /// Dequeuing under the start fence establishes an already-running input.
    /// No VM, log callback, transaction or store copy runs inside this fence.
    pub(super) fn running(self: &Arc<Self>, start: MutexGuard<'_, ()>) -> Running {
        assert!(
            !self.running.swap(true, Ordering::AcqRel),
            "single consumer VM"
        );
        drop(start);
        Running(self.clone())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::Release);
    }
}
