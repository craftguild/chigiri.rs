//! Safe Waker callbacks that observe whether a reviewed mutex is still held.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::task::{Wake, Waker};

#[derive(Clone)]
pub(super) struct ProbeCounts {
    drops: Arc<AtomicUsize>,
    wakes: Arc<AtomicUsize>,
    under_lock: Arc<AtomicUsize>,
}

impl ProbeCounts {
    pub(super) fn new() -> Self {
        Self {
            drops: Arc::new(AtomicUsize::new(0)),
            wakes: Arc::new(AtomicUsize::new(0)),
            under_lock: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(super) fn values(&self) -> (usize, usize, usize) {
        (
            self.drops.load(Ordering::SeqCst),
            self.wakes.load(Ordering::SeqCst),
            self.under_lock.load(Ordering::SeqCst),
        )
    }
}

struct LockProbe {
    locked: Arc<dyn Fn() -> bool + Send + Sync + 'static>,
    counts: ProbeCounts,
}

impl LockProbe {
    fn observe(&self, counter: &AtomicUsize) {
        counter.fetch_add(1, Ordering::SeqCst);
        if (self.locked)() {
            self.counts.under_lock.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl Wake for LockProbe {
    fn wake(self: Arc<Self>) {
        self.observe(&self.counts.wakes);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.observe(&self.counts.wakes);
    }
}

impl Drop for LockProbe {
    fn drop(&mut self) {
        self.observe(&self.counts.drops);
    }
}

pub(super) struct ProbeWaker {
    waker: Waker,
    life: Weak<LockProbe>,
}

impl ProbeWaker {
    pub(super) fn waker(&self) -> &Waker {
        &self.waker
    }

    pub(super) fn strong_count(&self) -> usize {
        self.life.strong_count()
    }
}

pub(super) fn probing_waker(
    locked: Arc<dyn Fn() -> bool + Send + Sync + 'static>,
    counts: ProbeCounts,
) -> ProbeWaker {
    let probe = Arc::new(LockProbe { locked, counts });
    let life = Arc::downgrade(&probe);
    ProbeWaker {
        waker: Waker::from(probe),
        life,
    }
}
