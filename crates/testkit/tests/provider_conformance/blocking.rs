//! Reject providers that block synchronously during method calls or polling.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use chigiri_rate::{
    OperationCost, QuotaKey, QuotaSubject, RateLimitDecision, RateLimitProviderError,
    RateLimitRequest, RateLimiter, TokenBucketPolicy,
};
use chigiri_testkit::assert_rate_provider_nonblocking_conformance;

#[path = "blocking/wake.rs"]
mod wake;

use wake::{
    DelayedSecondPollBlocking, ExcessiveWakeProvider, MultipleWakeProvider, PendingProvider,
    SecondPollBlocking, SpamSecondPollBlocking,
};

#[test]
fn limit_14_watchdog_rejects_method_and_poll_blocking_providers() {
    let oversized_called = Arc::new(AtomicBool::new(false));
    let oversized_dropped = Arc::new(AtomicBool::new(false));
    let oversized_key = key();
    assert!(
        assert_rate_provider_nonblocking_conformance(
            Arc::new(CallProbe {
                called: Arc::clone(&oversized_called),
                dropped: Arc::clone(&oversized_dropped),
            }),
            request(&oversized_key),
            Duration::MAX,
        )
        .is_err()
    );
    assert!(!oversized_called.load(Ordering::Acquire));
    assert!(oversized_dropped.load(Ordering::Acquire));

    let method_gate = Arc::new(Gate::default());
    reject_and_release(
        Arc::new(MethodBlocking(Arc::clone(&method_gate))),
        &method_gate,
    );

    let poll_gate = Arc::new(Gate::default());
    reject_and_release(Arc::new(PollBlocking(Arc::clone(&poll_gate))), &poll_gate);

    let repeated_poll_gate = Arc::new(Gate::default());
    reject_and_release(
        Arc::new(SecondPollBlocking(Arc::clone(&repeated_poll_gate))),
        &repeated_poll_gate,
    );

    let delayed_gate = Arc::new(Gate::default());
    let wake_done = Arc::new(Gate::default());
    reject_delayed_and_release(&delayed_gate, &wake_done);

    let spam_gate = Arc::new(Gate::default());
    let spam_stop = Arc::new(AtomicBool::new(false));
    let spam_done = Arc::new(Gate::default());
    reject_spam_and_release(&spam_gate, &spam_stop, &spam_done);

    let key = key();
    assert!(
        assert_rate_provider_nonblocking_conformance(
            Arc::new(PendingProvider),
            request(&key),
            Duration::from_millis(20),
        )
        .is_ok()
    );
    assert!(
        assert_rate_provider_nonblocking_conformance(
            Arc::new(MultipleWakeProvider),
            request(&key),
            Duration::from_millis(20),
        )
        .is_ok()
    );
    assert!(
        assert_rate_provider_nonblocking_conformance(
            Arc::new(ExcessiveWakeProvider),
            request(&key),
            Duration::from_millis(20),
        )
        .is_err()
    );
}

fn reject_and_release(provider: Arc<dyn RateLimiter>, gate: &Arc<Gate>) {
    let started = Instant::now();
    let key = key();
    assert!(
        assert_rate_provider_nonblocking_conformance(
            provider,
            request(&key),
            Duration::from_millis(20),
        )
        .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    gate.release_and_wait();
}

fn reject_delayed_and_release(gate: &Arc<Gate>, wake_done: &Arc<Gate>) {
    let key = key();
    let result = assert_rate_provider_nonblocking_conformance(
        Arc::new(DelayedSecondPollBlocking::new(
            Arc::clone(gate),
            Arc::clone(wake_done),
        )),
        request(&key),
        Duration::from_millis(20),
    );
    let entered = gate.entered.load(Ordering::Acquire);
    gate.release_and_wait();
    wake_done.wait_done();
    assert!(result.is_err());
    assert!(entered, "the second poll must reach its blocking gate");
}

fn reject_spam_and_release(gate: &Arc<Gate>, stop: &Arc<AtomicBool>, done: &Arc<Gate>) {
    let started = Instant::now();
    let key = key();
    assert!(
        assert_rate_provider_nonblocking_conformance(
            Arc::new(SpamSecondPollBlocking {
                gate: Arc::clone(gate),
                stop: Arc::clone(stop),
                done: Arc::clone(done),
            }),
            request(&key),
            Duration::from_millis(20),
        )
        .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    stop.store(true, Ordering::Release);
    done.wait_done();
    gate.release_and_wait();
}

fn key() -> QuotaKey {
    QuotaKey::new(b"blocking".to_vec(), QuotaSubject::Missing, None).unwrap()
}

fn request(key: &QuotaKey) -> RateLimitRequest<'_> {
    let policy = TokenBucketPolicy::new(1, 1, Duration::from_secs(1)).unwrap();
    RateLimitRequest::new(key, OperationCost::one(), policy.into())
}

#[derive(Default)]
struct Gate {
    entered: AtomicBool,
    released: Mutex<bool>,
    release: Condvar,
    done: Mutex<bool>,
    completed: Condvar,
}

impl Gate {
    fn block(&self) {
        self.entered.store(true, Ordering::Release);
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.release.wait(released).unwrap();
        }
    }

    fn finish(&self) {
        *self.done.lock().unwrap() = true;
        self.completed.notify_all();
    }

    fn release_and_wait(&self) {
        *self.released.lock().unwrap() = true;
        self.release.notify_all();
        self.wait_done();
    }

    fn wait_done(&self) {
        let done = self.done.lock().unwrap();
        let (done, timeout) = self
            .completed
            .wait_timeout_while(done, Duration::from_secs(1), |done| !*done)
            .unwrap();
        assert!(*done && !timeout.timed_out());
    }
}

struct MethodBlocking(Arc<Gate>);

struct CallProbe {
    called: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}

impl Drop for CallProbe {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}

impl RateLimiter for CallProbe {
    fn check_and_consume<'a>(
        &'a self,
        _: RateLimitRequest<'a>,
    ) -> chigiri_rate::RateLimitFuture<'a> {
        self.called.store(true, Ordering::Release);
        Box::pin(std::future::pending())
    }
}

impl Drop for MethodBlocking {
    fn drop(&mut self) {
        self.0.finish();
    }
}

impl RateLimiter for MethodBlocking {
    fn check_and_consume<'a>(
        &'a self,
        _: RateLimitRequest<'a>,
    ) -> chigiri_rate::RateLimitFuture<'a> {
        self.0.block();
        Box::pin(std::future::pending())
    }
}

struct PollBlocking(Arc<Gate>);

impl Drop for PollBlocking {
    fn drop(&mut self) {
        self.0.finish();
    }
}

impl RateLimiter for PollBlocking {
    fn check_and_consume<'a>(
        &'a self,
        _: RateLimitRequest<'a>,
    ) -> chigiri_rate::RateLimitFuture<'a> {
        Box::pin(BlockingFuture(Arc::clone(&self.0)))
    }
}

struct BlockingFuture(Arc<Gate>);

impl Future for BlockingFuture {
    type Output = Result<RateLimitDecision, RateLimitProviderError>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.block();
        Poll::Pending
    }
}
