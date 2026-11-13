//! Establish one absolute recovery deadline shared by every server owner.

use std::sync::Arc;
use std::time::Instant;

use chigiri_mock::cancellation::Cancellation;
use tokio::sync::watch;

#[derive(Clone)]
pub(super) struct Shutdown {
    deadline: watch::Sender<Option<Instant>>,
    native: watch::Sender<Option<Instant>>,
    worker: Cancellation,
    writer: Arc<dyn Fn(Instant) + Send + Sync>,
    events: Arc<dyn Fn() + Send + Sync>,
    events_running: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl Shutdown {
    pub(super) fn new(
        worker: Cancellation,
        writer: impl Fn(Instant) + Send + Sync + 'static,
    ) -> Self {
        let (deadline, _) = watch::channel(None);
        let (native, _) = watch::channel(None);
        Self {
            deadline,
            native,
            worker,
            writer: Arc::new(writer),
            events: Arc::new(|| {}),
            events_running: Arc::new(|| false),
        }
    }

    pub(super) fn with_event_stop(
        mut self,
        stop: impl Fn() + Send + Sync + 'static,
        running: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        self.events = Arc::new(stop);
        self.events_running = Arc::new(running);
        self
    }

    pub(super) async fn finish_events(&self, deadline: Instant) -> bool {
        while (self.events_running)() {
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        true
    }

    pub(super) fn subscribe(&self) -> watch::Receiver<Option<Instant>> {
        self.deadline.subscribe()
    }

    pub(super) fn begin(&self) -> Instant {
        let mut first = false;
        self.deadline.send_if_modified(|deadline| {
            if deadline.is_none() {
                *deadline = Some(Instant::now() + chigiri_mock::limits::SHUTDOWN_TIME);
                first = true;
            }
            first
        });
        let deadline = self
            .deadline
            .borrow()
            .expect("established shutdown deadline");
        if first {
            (self.events)();
            (self.writer)(deadline);
        }
        deadline
    }

    pub(super) fn begin_native(&self) -> Instant {
        let deadline = self.begin();
        let first = self.native.send_if_modified(|native| {
            if native.is_some() {
                return false;
            }
            *native = Some(deadline);
            true
        });
        if first {
            self.worker.cancel();
        }
        deadline
    }

    pub(super) async fn wait_native(&self) -> Instant {
        let mut receiver = self.native.subscribe();
        let deadline = receiver
            .wait_for(Option::is_some)
            .await
            .expect("native shutdown owner");
        deadline.expect("established native shutdown deadline")
    }

    pub(super) async fn wait(&self) -> Instant {
        let mut receiver = self.subscribe();
        let deadline = receiver
            .wait_for(Option::is_some)
            .await
            .expect("shutdown owner");
        deadline.expect("established shutdown deadline")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn every_owner_observes_one_original_deadline_and_one_cancellation() {
        let worker = Cancellation::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let shutdown = Shutdown::new(worker.clone(), move |_| {
            called.fetch_add(1, Ordering::SeqCst);
        });
        let waiting = shutdown.clone();
        let observer = tokio::spawn(async move { waiting.wait().await });
        tokio::task::yield_now().await;
        assert!(!worker.cancelled());
        let original = shutdown.begin();
        assert_eq!(observer.await.unwrap(), original);
        assert_eq!(shutdown.clone().begin(), original);
        assert_eq!(shutdown.wait().await, original);
        assert!(!worker.cancelled());
        assert_eq!(shutdown.begin_native(), original);
        assert_eq!(shutdown.wait_native().await, original);
        assert_eq!(shutdown.clone().begin_native(), original);
        assert!(worker.cancelled());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
