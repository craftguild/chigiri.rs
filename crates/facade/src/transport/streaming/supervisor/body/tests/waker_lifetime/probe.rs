//! Safe reentrant Waker disposal and wake probes for the body mailbox.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, TryLockError, Weak};
use std::task::{Wake, Waker};

use super::super::super::BodyPort;

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
    port: Arc<BodyPort>,
    counts: ProbeCounts,
}

impl LockProbe {
    fn observe(&self, counter: &AtomicUsize) {
        counter.fetch_add(1, Ordering::SeqCst);
        match self.port.state.try_lock() {
            Ok(guard) => drop(guard),
            Err(TryLockError::Poisoned(poisoned)) => drop(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => {
                self.counts.under_lock.fetch_add(1, Ordering::SeqCst);
            }
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

    pub(super) fn into_waker(self) -> Waker {
        self.waker
    }
}

pub(super) fn probing_waker(port: Arc<BodyPort>, counts: ProbeCounts) -> ProbeWaker {
    let probe = Arc::new(LockProbe { port, counts });
    let life = Arc::downgrade(&probe);
    ProbeWaker {
        waker: Waker::from(probe),
        life,
    }
}
