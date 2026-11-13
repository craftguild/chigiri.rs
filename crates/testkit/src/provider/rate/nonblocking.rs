//! Bounded-event monitoring for rate-provider method calls and future polling.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use chigiri_rate::{RateLimitRequest, RateLimiter};

use crate::provider::ProviderConformanceError;

/// Verifies that each provider method call and each future poll requested by a
/// wake returns within the watchdog.
pub fn assert_rate_provider_nonblocking_conformance(
    provider: Arc<dyn RateLimiter>,
    request: RateLimitRequest<'_>,
    watchdog: Duration,
) -> Result<(), ProviderConformanceError> {
    if watchdog.is_zero() {
        return Err(ProviderConformanceError::new(
            "rate provider watchdog must be positive",
        ));
    }
    let initial_phase = Instant::now();
    if initial_phase.checked_add(watchdog).is_none() {
        return Err(ProviderConformanceError::new(
            "rate provider watchdog is too large",
        ));
    }
    let key = request.key().clone();
    let cost = request.cost();
    let policy = request.policy();
    let (event_sender, event_receiver) = mpsc::sync_channel(1);
    let stopped = Arc::new(AtomicBool::new(false));
    let worker_stopped = Arc::clone(&stopped);
    let worker_events = event_sender.clone();
    let worker = std::thread::spawn(move || {
        let request = RateLimitRequest::new(&key, cost, policy);
        let mut future = provider.check_and_consume(request);
        let (wake_sender, wake_receiver) = mpsc::sync_channel(1);
        let waker = std::task::Waker::from(Arc::new(WakeSignal {
            worker: wake_sender,
            observer: worker_events.clone(),
        }));
        let mut context = std::task::Context::from_waker(&waker);
        for _ in 0..64 {
            if worker_stopped.load(Ordering::Acquire)
                || worker_events.send(WorkerEvent::PollStarted).is_err()
            {
                return;
            }
            if future.as_mut().poll(&mut context).is_ready() {
                let _ = worker_events.send(WorkerEvent::Conformant);
                return;
            }
            match wake_receiver.try_recv() {
                Ok(()) => continue,
                Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if worker_events.send(WorkerEvent::PollPending).is_err()
                || !wait_for_wake(&wake_receiver, &worker_stopped)
            {
                return;
            }
        }
        let _ = worker_events.send(WorkerEvent::Rejected);
    });
    drop(event_sender);
    observe_worker(event_receiver, stopped, worker, watchdog, initial_phase)
}

#[derive(Clone, Copy)]
enum WorkerEvent {
    PollStarted,
    PollPending,
    WakeObserved,
    Conformant,
    Rejected,
}

struct WakeSignal {
    worker: mpsc::SyncSender<()>,
    observer: mpsc::SyncSender<WorkerEvent>,
}

impl std::task::Wake for WakeSignal {
    fn wake(self: Arc<Self>) {
        self.signal();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.signal();
    }
}

impl WakeSignal {
    fn signal(&self) {
        let _ = self.observer.try_send(WorkerEvent::WakeObserved);
        let _ = self.worker.try_send(());
    }
}

fn wait_for_wake(receiver: &mpsc::Receiver<()>, stopped: &AtomicBool) -> bool {
    while !stopped.load(Ordering::Acquire) {
        match receiver.recv_timeout(Duration::from_millis(1)) {
            Ok(()) => return true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        }
    }
    false
}

fn observe_worker(
    receiver: mpsc::Receiver<WorkerEvent>,
    stopped: Arc<AtomicBool>,
    worker: std::thread::JoinHandle<()>,
    watchdog: Duration,
    initial_phase: Instant,
) -> Result<(), ProviderConformanceError> {
    let mut idle = false;
    let mut phase_started = initial_phase;
    loop {
        let elapsed = phase_started.elapsed();
        if elapsed >= watchdog {
            return finish_timeout(idle, receiver, stopped, worker);
        }
        match receiver.recv_timeout(watchdog.saturating_sub(elapsed)) {
            Ok(WorkerEvent::PollPending) => {
                idle = true;
                phase_started = Instant::now();
            }
            Ok(WorkerEvent::PollStarted) => {
                idle = false;
                phase_started = Instant::now();
            }
            Ok(WorkerEvent::WakeObserved) if idle => {
                idle = false;
                phase_started = Instant::now();
            }
            Ok(WorkerEvent::WakeObserved) => {}
            Ok(WorkerEvent::Conformant) => return join_worker(stopped, worker),
            Ok(WorkerEvent::Rejected) => {
                join_worker(stopped, worker)?;
                return Err(ProviderConformanceError::new(
                    "rate provider future did not become quiescent",
                ));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return finish_timeout(idle, receiver, stopped, worker);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return join_worker(stopped, worker);
            }
        }
    }
}

fn finish_timeout(
    idle: bool,
    receiver: mpsc::Receiver<WorkerEvent>,
    stopped: Arc<AtomicBool>,
    worker: std::thread::JoinHandle<()>,
) -> Result<(), ProviderConformanceError> {
    stopped.store(true, Ordering::Release);
    drop(receiver);
    if idle {
        return join_worker(stopped, worker);
    }
    Err(ProviderConformanceError::new(
        "rate provider method or future poll blocked the worker",
    ))
}

fn join_worker(
    stopped: Arc<AtomicBool>,
    worker: std::thread::JoinHandle<()>,
) -> Result<(), ProviderConformanceError> {
    stopped.store(true, Ordering::Release);
    worker
        .join()
        .map_err(|_| ProviderConformanceError::new("rate provider worker panicked"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_timeout_closes_the_full_channel_before_joining_the_worker() {
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.send(WorkerEvent::WakeObserved).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = Arc::clone(&stopped);
        let (ready_sender, ready_receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let should_send = !worker_stopped.load(Ordering::Acquire);
            ready_sender.send(()).unwrap();
            if should_send {
                let _ = sender.send(WorkerEvent::PollStarted);
            }
        });
        ready_receiver.recv().unwrap();
        std::thread::sleep(Duration::from_millis(5));
        assert!(!worker.is_finished());
        let (done_sender, done_receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = done_sender.send(finish_timeout(true, receiver, stopped, worker));
        });
        assert!(
            done_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .is_ok()
        );
    }
}
