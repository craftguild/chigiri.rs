//! Join both fixed worker responsibilities before releasing their source ownership.

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{Client, Worker, environment};
use crate::luau::ScriptError;

impl Worker {
    /// Stop event admission, finish the already-running consumer within the same
    /// deadline, then cancel and physically join both fixed worker threads.
    pub async fn stop_graceful(&mut self, deadline: Instant) -> Result<(), ScriptError> {
        self.client.quiesce_events();
        while self.client.events.is_running()
            && !self.client.shutdown.cancelled()
            && Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        self.stop(deadline).await
    }

    /// Complete cancellation and both physical joins within the HTTP shutdown deadline.
    pub async fn stop(&mut self, deadline: Instant) -> Result<(), ScriptError> {
        self.client.shutdown.cancel();
        while self.running() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let late = Instant::now() >= deadline;
        let joined = self.join();
        let closed = self.client.close_publications();
        joined?;
        closed?;
        if late {
            Err(environment("worker exceeded shutdown deadline"))
        } else {
            Ok(())
        }
    }

    fn running(&self) -> bool {
        self.thread
            .iter()
            .chain(self.delivery.iter())
            .any(|thread| !thread.is_finished())
    }

    pub(super) fn join(&mut self) -> Result<(), ScriptError> {
        // Consume both handles even when either thread failed. Returning early
        // would detach the peer and falsely release its physical ownership.
        let vm = join(self.thread.take(), "VM worker failed");
        let delivery = join(self.delivery.take(), "source delivery worker failed");
        vm.and(delivery)
    }
}

fn join(thread: Option<JoinHandle<()>>, message: &'static str) -> Result<(), ScriptError> {
    match thread {
        Some(thread) => thread.join().map_err(|_| environment(message)),
        None => Ok(()),
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.client.shutdown.cancel();
        let _ = self.join();
        let _ = self.client.close_publications();
    }
}

impl Client {
    /// Whether an already-started named callback still owns its input/candidate.
    pub fn events_running(&self) -> bool {
        self.events.is_running()
    }

    /// End generation and future consumer callbacks without cancelling the
    /// already-running candidate. The host must still call the Worker stop/join.
    pub fn quiesce_events(&self) {
        self.events.stop();
        if let Ok(store) = self.committed.lock() {
            store.delivery_wake().notify();
        }
    }

    pub(super) fn close_publications(&self) -> Result<(), ScriptError> {
        let (staging, retired) = {
            let mut committed = self
                .committed
                .lock()
                .map_err(|_| environment("store owner poisoned"))?;
            (committed.staging(), committed.close())
        };
        // Retiring captures and topic metadata never runs under the store's
        // linearization lock or the request cancellation fence.
        let result = staging
            .close()
            .map_err(|_| environment("publication staging poisoned"));
        drop(retired);
        result
    }
}
