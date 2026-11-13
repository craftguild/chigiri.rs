//! Coordinate cancellation and commit fencing under one lock.

use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Default)]
/// Token that coordinates worker cancellation and commit ordering under one lock.
pub struct Cancellation {
    signal: Arc<Signal>,
    linked: Arc<[Arc<Signal>]>,
}

#[derive(Default)]
struct Signal {
    value: Mutex<bool>,
    changed: tokio::sync::Notify,
}

impl Cancellation {
    /// Fixed allocations owned by a fresh token, excluding existing linked tokens.
    pub(crate) const fn retained_allocation_bytes() -> usize {
        size_of::<Signal>() + 4 * size_of::<usize>()
    }

    /// Permanently cancel subsequent execution and commit attempts.
    pub fn cancel(&self) {
        *self.signal.value.lock().expect("cancellation lock") = true;
        self.signal.changed.notify_waiters();
    }
    /// Return whether cancellation has been triggered; cancellation is sticky.
    pub fn cancelled(&self) -> bool {
        self.flags()
            .any(|flag| *flag.value.lock().expect("cancellation lock"))
    }

    /// Wait for sticky cancellation from this token or any linked token.
    pub(crate) async fn wait(&self) {
        use std::future::Future;
        use std::task::Poll;
        let mut waits: Vec<_> = self
            .flags()
            .map(|flag| Box::pin(flag.changed.notified()))
            .collect();
        std::future::poll_fn(|cx| {
            for wait in &mut waits {
                let _ = wait.as_mut().poll(cx);
            }
            if self.cancelled() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }

    /// Return a token cancelled when either this token or its parent is cancelled.
    pub(crate) fn linked(&self, parent: &Self) -> Self {
        let mut linked: Vec<_> = self.linked.iter().cloned().collect();
        linked.extend(parent.flags().cloned());
        linked.sort_by_key(Arc::as_ptr);
        linked.dedup_by(|a, b| Arc::ptr_eq(a, b));
        linked.retain(|flag| !Arc::ptr_eq(flag, &self.signal));
        // Normal request, job, and shutdown linking uses at most three signals.
        if linked.len() > 4 {
            let cancelled = Self::default();
            cancelled.cancel();
            return cancelled;
        }
        Self {
            signal: self.signal.clone(),
            linked: linked.into(),
        }
    }

    fn flags(&self) -> impl Iterator<Item = &Arc<Signal>> {
        std::iter::once(&self.signal).chain(self.linked.iter())
    }

    /// Atomically decide whether a test-only commit may proceed after final validation.
    /// The callback must not perform I/O, await, cancel this token, or run rollback cleanup
    /// because cancellation locks remain held until the callback returns.
    #[cfg(test)]
    pub(crate) fn commit_if_active<T>(&self, commit: impl FnOnce() -> T) -> Option<T> {
        self.locked(None, None, commit)
    }

    /// Commit only when neither request cancellation, shutdown, nor the deadline blocks it.
    pub fn commit<T>(
        &self,
        shutdown: &Self,
        deadline: Instant,
        commit: impl FnOnce() -> T,
    ) -> Option<T> {
        self.locked(Some(shutdown), Some(deadline), commit)
    }

    fn locked<T>(
        &self,
        other: Option<&Self>,
        deadline: Option<Instant>,
        commit: impl FnOnce() -> T,
    ) -> Option<T> {
        // Lock each distinct signal once in address order to avoid deadlocks with concurrent cancellation.
        let mut flags: Vec<_> = self.flags().collect();
        if let Some(other) = other {
            flags.extend(other.flags());
        }
        flags.sort_by_key(|flag| Arc::as_ptr(flag));
        flags.dedup_by(|a, b| Arc::ptr_eq(a, b));
        let locks: Vec<_> = flags
            .iter()
            .map(|flag| flag.value.lock().expect("cancellation lock"))
            .collect();
        if locks.iter().any(|lock| **lock)
            || deadline.is_some_and(|deadline| Instant::now() >= deadline)
        {
            None
        } else {
            Some(commit())
        }
    }
}

/// Guard that cancels its token when dropped.
pub struct CancelOnDrop(pub Cancellation);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linked_cancellation_wakes_registered_waiters_and_remains_sticky() {
        use std::future::Future;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::task::{Context, Wake, Waker};
        struct Count(AtomicUsize);
        impl Wake for Count {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let parent = Cancellation::default();
        let child = Cancellation::default().linked(&parent);
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let waker = Waker::from(count.clone());
        let mut context = Context::from_waker(&waker);
        let mut wait = Box::pin(child.wait());
        assert!(wait.as_mut().poll(&mut context).is_pending());
        parent.cancel();
        assert!(count.0.load(Ordering::SeqCst) > 0);
        assert!(wait.as_mut().poll(&mut context).is_ready());
        assert!(
            Box::pin(child.wait())
                .as_mut()
                .poll(&mut context)
                .is_ready()
        );
    }

    #[test]
    fn hk_14_job_completion_preserves_parent_but_parent_cancellation_prevents_commit() {
        let parent = Cancellation::default();
        let child = Cancellation::default().linked(&parent);
        child.cancel();
        assert!(child.cancelled());
        assert!(!parent.cancelled());
        let child = Cancellation::default().linked(&parent);
        let shutdown = Cancellation::default();
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        assert_eq!(child.commit(&shutdown, deadline, || 7), Some(7));
        parent.cancel();
        assert!(child.cancelled());
        assert_eq!(child.commit(&shutdown, deadline, || 8), None);
        assert_eq!(child.commit_if_active(|| 9), None);
        assert_eq!(child.commit(&child, deadline, || 10), None);
    }
    #[test]
    fn mock_12_cancel_and_commit_have_one_ordered_commit_point() {
        let root = Cancellation::default();
        let shutdown = Cancellation::default();
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        let mut store = 0;
        root.cancel();
        assert!(root.commit(&shutdown, deadline, || store = 1).is_none());
        assert_eq!(store, 0);
        let root = Cancellation::default();
        assert!(root.commit(&shutdown, deadline, || store = 2).is_some());
        root.cancel();
        assert_eq!(store, 2);
        let root = Cancellation::default();
        shutdown.cancel();
        assert!(root.commit(&shutdown, deadline, || store = 3).is_none());
        assert_eq!(store, 2);
    }

    #[test]
    fn mock_12_cancel_racing_commit_cannot_split_the_swap_and_expired_commit_is_rejected() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let root = Cancellation::default();
        let shutdown = Cancellation::default();
        let store = AtomicUsize::new(0);
        let (entered, locked) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let (cancelled, completed) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let (root, shutdown, store) = (&root, &shutdown, &store);
            let commit = scope.spawn(move || {
                root.commit(
                    shutdown,
                    Instant::now() + std::time::Duration::from_secs(2),
                    || {
                        entered.send(()).unwrap();
                        released.recv().unwrap();
                        store.store(1, Ordering::SeqCst);
                    },
                )
            });
            locked
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            scope.spawn(|| {
                root.cancel();
                cancelled.send(()).unwrap();
            });
            assert!(
                completed
                    .recv_timeout(std::time::Duration::from_millis(10))
                    .is_err()
            );
            release.send(()).unwrap();
            assert!(commit.join().unwrap().is_some());
            completed
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
        });
        assert_eq!(store.load(Ordering::SeqCst), 1);
        let root = Cancellation::default();
        assert!(
            root.commit(
                &shutdown,
                Instant::now() - std::time::Duration::from_millis(1),
                || store.store(2, Ordering::SeqCst)
            )
            .is_none()
        );
        assert_eq!(store.load(Ordering::SeqCst), 1);
    }
}
