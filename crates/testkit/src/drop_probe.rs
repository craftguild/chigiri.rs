//! Deterministic probe for observing timeout-driven future cancellation without spawning.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};

/// Thread-safe counters for poll and drop observations.
#[derive(Debug, Default)]
pub struct DropProbeState {
    polls: AtomicUsize,
    dropped: AtomicBool,
}

impl DropProbeState {
    pub fn polls(&self) -> usize {
        self.polls.load(Ordering::SeqCst)
    }
    pub fn was_dropped(&self) -> bool {
        self.dropped.load(Ordering::SeqCst)
    }
}

/// Future that remains pending until it is dropped.
pub struct PendingDropProbe {
    state: Arc<DropProbeState>,
}

impl PendingDropProbe {
    pub fn new() -> (Self, Arc<DropProbeState>) {
        let state = Arc::new(DropProbeState::default());
        (
            Self {
                state: Arc::clone(&state),
            },
            state,
        )
    }
}

impl Future for PendingDropProbe {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        self.state.polls.fetch_add(1, Ordering::SeqCst);
        Poll::Pending
    }
}

impl Drop for PendingDropProbe {
    fn drop(&mut self) {
        self.state.dropped.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    use super::PendingDropProbe;

    #[test]
    fn observes_polling_and_dropping_a_pending_future() {
        let (probe, state) = PendingDropProbe::new();
        let mut probe = Box::pin(probe);
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        assert_eq!(probe.as_mut().poll(&mut context), Poll::Pending);
        drop(probe);
        assert_eq!(state.polls(), 1);
        assert!(state.was_dropped());
    }
}
