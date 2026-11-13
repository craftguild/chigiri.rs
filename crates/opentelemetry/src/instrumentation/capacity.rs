//! Physical scope capacity, released only when its owner is actually destroyed.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) struct Permit(Arc<AtomicUsize>);

impl Permit {
    pub(super) fn acquire(active: &Arc<AtomicUsize>, limit: usize) -> Option<Self> {
        active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < limit).then(|| count + 1)
            })
            .ok()
            .map(|_| Self(Arc::clone(active)))
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
