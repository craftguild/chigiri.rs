//! Capacity-one handoff tied to one application-owned HTTP connection.

use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use chigiri::__streaming::HttpStreamingError;

struct Shared<T> {
    state: Mutex<State<T>>,
}

struct State<T> {
    open: bool,
    occupied: bool,
    pending: Option<T>,
    driver: Option<Waker>,
}

struct RegistrationLife<T> {
    shared: Arc<Shared<T>>,
}

impl<T> Drop for RegistrationLife<T> {
    fn drop(&mut self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.open = false;
        let wake = state.driver.take();
        drop(state);
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}

pub(super) struct Registration<T> {
    life: Arc<RegistrationLife<T>>,
}

impl<T> Clone for Registration<T> {
    fn clone(&self) -> Self {
        Self {
            life: Arc::clone(&self.life),
        }
    }
}

pub(super) struct Receiver<T> {
    shared: Arc<Shared<T>>,
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.open = false;
        let pending = state.pending.take();
        let driver = state.driver.take();
        drop(state);
        drop(driver);
        drop(pending);
    }
}

pub(super) fn pair<T>() -> (Registration<T>, Receiver<T>) {
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            open: true,
            occupied: false,
            pending: None,
            driver: None,
        }),
    });
    (
        Registration {
            life: Arc::new(RegistrationLife {
                shared: Arc::clone(&shared),
            }),
        },
        Receiver { shared },
    )
}

impl<T> Registration<T> {
    pub(super) fn attach(&self, entry: T) -> Result<(), HttpStreamingError> {
        let mut state = self
            .life
            .shared
            .state
            .lock()
            .map_err(|_| HttpStreamingError::Invariant)?;
        if !state.open {
            return Err(HttpStreamingError::Closed);
        }
        if state.occupied || state.pending.is_some() {
            return Err(HttpStreamingError::Capacity);
        }
        state.occupied = true;
        state.pending = Some(entry);
        let wake = state.driver.take();
        drop(state);
        if let Some(wake) = wake {
            wake.wake();
        }
        Ok(())
    }
}

impl<T> Receiver<T> {
    pub(super) fn poll_take(
        &self,
        context: &mut Context<'_>,
    ) -> Poll<Result<Option<T>, HttpStreamingError>> {
        let replacement = context.waker().clone();
        let mut state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                drop(poisoned);
                drop(replacement);
                return Poll::Ready(Err(HttpStreamingError::Invariant));
            }
        };
        if let Some(entry) = state.pending.take() {
            drop(state);
            drop(replacement);
            return Poll::Ready(Ok(Some(entry)));
        }
        if !state.open {
            drop(state);
            drop(replacement);
            return Poll::Ready(Ok(None));
        }
        let previous = state.driver.replace(replacement);
        drop(state);
        drop(previous);
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Barrier, mpsc};
    use std::task::{Wake, Waker};

    mod probe;

    use super::*;
    use probe::{ProbeCounts, probing_waker};

    struct Counter(AtomicUsize);

    impl Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn attach_is_capacity_one_and_wakes_the_driver() {
        let (registration, receiver) = pair::<()>();
        let counts = ProbeCounts::new();
        let wake = probing_waker(Arc::clone(&receiver.shared), counts.clone());
        let mut context = Context::from_waker(wake.waker());
        assert!(receiver.poll_take(&mut context).is_pending());

        registration.attach(()).unwrap();
        assert_eq!(counts.values(), (0, 1, 0));
        assert_eq!(receiver.poll_take(&mut context), Poll::Ready(Ok(Some(()))));
        assert_eq!(registration.attach(()), Err(HttpStreamingError::Capacity));
        drop(wake);
        assert_eq!(counts.values(), (1, 1, 0));
    }

    #[test]
    fn last_registration_drop_finishes_an_unused_receiver() {
        let (registration, receiver) = pair::<()>();
        let clone = registration.clone();
        drop(registration);
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        assert!(receiver.poll_take(&mut context).is_pending());
        drop(clone);
        assert_eq!(receiver.poll_take(&mut context), Poll::Ready(Ok(None)));
    }

    #[test]
    fn attach_from_another_clone_wakes_the_waiting_driver() {
        let (registration, receiver) = pair();
        let producer = registration.clone();
        let wake = Arc::new(Counter(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wake));
        let mut context = Context::from_waker(&waker);
        assert!(receiver.poll_take(&mut context).is_pending());
        let barrier = Arc::new(Barrier::new(2));
        let worker_barrier = Arc::clone(&barrier);
        let (sent, received) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            worker_barrier.wait();
            producer.attach(11).unwrap();
            sent.send(()).unwrap();
        });
        barrier.wait();
        received.recv().unwrap();
        worker.join().unwrap();
        assert_eq!(wake.0.load(Ordering::SeqCst), 1);
        assert_eq!(receiver.poll_take(&mut context), Poll::Ready(Ok(Some(11))));
        drop(registration);
    }

    #[test]
    fn poisoned_port_fails_closed_and_drops_the_entry() {
        struct DropProbe(Arc<AtomicUsize>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let (registration, receiver) = pair();
        let drops = Arc::new(AtomicUsize::new(0));
        let shared = Arc::clone(&receiver.shared);
        let panic = catch_unwind(AssertUnwindSafe(move || {
            let _guard = shared.state.lock().unwrap();
            panic!("poison the connection-local port");
        }));
        assert!(panic.is_err());
        assert!(matches!(
            registration.attach(DropProbe(Arc::clone(&drops))),
            Err(HttpStreamingError::Invariant)
        ));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        assert!(matches!(
            receiver.poll_take(&mut context),
            Poll::Ready(Err(HttpStreamingError::Invariant))
        ));
    }

    #[test]
    fn driver_drop_closes_admission_and_rejects_a_late_owner() {
        struct DropProbe(Arc<AtomicUsize>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let (registration, receiver) = pair();
        drop(receiver);
        let drops = Arc::new(AtomicUsize::new(0));
        assert!(matches!(
            registration.attach(DropProbe(Arc::clone(&drops))),
            Err(HttpStreamingError::Closed)
        ));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn poisoned_poll_drops_the_replacement_only_after_unlock() {
        let (registration, receiver) = pair::<()>();
        let shared = Arc::clone(&receiver.shared);
        let panic = catch_unwind(AssertUnwindSafe(move || {
            let _guard = shared.state.lock().unwrap();
            panic!("poison the connection-local port");
        }));
        assert!(panic.is_err());

        let counts = ProbeCounts::new();
        let replacement = probing_waker(Arc::clone(&receiver.shared), counts.clone());
        let mut context = Context::from_waker(replacement.waker());
        assert!(matches!(
            receiver.poll_take(&mut context),
            Poll::Ready(Err(HttpStreamingError::Invariant))
        ));
        assert_eq!(replacement.strong_count(), 1);
        assert_eq!(counts.values(), (0, 0, 0));
        drop(replacement);
        assert_eq!(counts.values(), (1, 0, 0));
        drop(receiver);
        drop(registration);
    }

    #[test]
    fn replacing_a_waker_clones_and_drops_only_outside_the_mutex() {
        let (registration, receiver) = pair::<()>();
        let counts = ProbeCounts::new();
        let old = probing_waker(Arc::clone(&receiver.shared), counts.clone());
        receiver.shared.state.lock().unwrap().driver = Some(old.into_waker());
        let replacement = probing_waker(Arc::clone(&receiver.shared), counts.clone());
        {
            let mut context = Context::from_waker(replacement.waker());
            assert!(receiver.poll_take(&mut context).is_pending());
        }
        assert_eq!(replacement.strong_count(), 2);
        assert_eq!(counts.values(), (1, 0, 0));
        drop(receiver);
        assert_eq!(replacement.strong_count(), 1);
        assert_eq!(counts.values(), (1, 0, 0));
        drop(replacement);
        assert_eq!(counts.values(), (2, 0, 0));
        drop(registration);
    }

    #[test]
    fn receiver_drop_releases_the_stored_waker_only_after_unlock() {
        let (registration, receiver) = pair::<()>();
        let counts = ProbeCounts::new();
        let stored = probing_waker(Arc::clone(&receiver.shared), counts.clone());
        receiver.shared.state.lock().unwrap().driver = Some(stored.into_waker());
        drop(receiver);
        assert_eq!(counts.values(), (1, 0, 0));
        drop(registration);
    }
}
