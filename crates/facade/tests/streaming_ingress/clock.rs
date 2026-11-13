use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use chigiri::{BoxFuture, DeadlineFactory};

#[derive(Default)]
pub struct TrackingDeadlines {
    created: Mutex<Vec<Duration>>,
    live: Arc<Mutex<BTreeMap<Duration, usize>>>,
}

impl TrackingDeadlines {
    pub fn created(&self, duration: Duration) -> usize {
        self.created
            .lock()
            .unwrap()
            .iter()
            .filter(|value| **value == duration)
            .count()
    }

    pub fn live(&self, duration: Duration) -> usize {
        self.live
            .lock()
            .unwrap()
            .get(&duration)
            .copied()
            .unwrap_or(0)
    }
}

impl DeadlineFactory for TrackingDeadlines {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        self.created.lock().unwrap().push(duration);
        *self.live.lock().unwrap().entry(duration).or_default() += 1;
        Box::pin(TrackedDeadline {
            duration,
            live: Arc::clone(&self.live),
        })
    }
}

struct TrackedDeadline {
    duration: Duration,
    live: Arc<Mutex<BTreeMap<Duration, usize>>>,
}

impl Future for TrackedDeadline {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for TrackedDeadline {
    fn drop(&mut self) {
        let mut live = self.live.lock().unwrap();
        let count = live.get_mut(&self.duration).unwrap();
        *count -= 1;
    }
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    struct Signal(std::thread::Thread);

    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(Signal(std::thread::current())));
    let mut task = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut task) {
            Poll::Ready(result) => return result,
            Poll::Pending => std::thread::park(),
        }
    }
}
