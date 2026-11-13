//! Conformance for rate-provider atomicity, metadata, and clock behavior.

use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use chigiri_rate::{
    MonotonicTick, RateLimitDecision, RateLimitFuture, RateLimitProviderError,
    RateLimitRejectReason, RateLimitRequest, RateLimiter,
};

use super::ProviderConformanceError;

mod nonblocking;
mod state;

pub use nonblocking::assert_rate_provider_nonblocking_conformance;
pub use state::{RateProviderStateCase, assert_rate_provider_state_conformance};

/// Wrapper that exposes the provider's atomic method while preserving borrow lifetimes.
pub struct RateProviderConformance<'a> {
    provider: &'a dyn RateLimiter,
}

impl<'a> RateProviderConformance<'a> {
    pub const fn new(provider: &'a dyn RateLimiter) -> Self {
        Self { provider }
    }

    pub fn check_and_consume<'b>(&'b self, request: RateLimitRequest<'b>) -> RateLimitFuture<'b> {
        self.provider.check_and_consume(request)
    }
}

/// Closed expected decision for a rate-provider case.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpectedRateDecision {
    Allowed,
    Rejected,
}

/// One rate-provider decision fixture.
#[derive(Clone, Copy, Debug)]
pub struct RateProviderCase<'a> {
    pub request: RateLimitRequest<'a>,
    pub expected: ExpectedRateDecision,
}

/// Full-bucket atomicity fixture whose requests are polled concurrently.
#[derive(Clone, Copy, Debug)]
pub struct RateProviderConcurrencyCase<'a> {
    pub request: RateLimitRequest<'a>,
    pub attempts: usize,
}

/// Testkit hooks for a provider's clock and retained-key count.
pub trait RateProviderTimeControl: Send + Sync {
    fn set_now(&self, tick: MonotonicTick);
    fn entry_count(&self) -> Result<usize, RateLimitProviderError>;
}

/// Fixture for clock reversal and idle expiry.
#[derive(Clone, Copy, Debug)]
pub struct RateProviderTimeCase<'a> {
    pub primary: RateLimitRequest<'a>,
    pub secondary: RateLimitRequest<'a>,
    pub initial: MonotonicTick,
    pub rollback: MonotonicTick,
    pub before_expiry: MonotonicTick,
    pub after_primary_expiry: MonotonicTick,
    pub after_secondary_expiry: MonotonicTick,
}

/// Applies the metadata integration contract to a third-party rate provider.
pub async fn assert_rate_provider_conformance(
    provider: &dyn RateLimiter,
    cases: &[RateProviderCase<'_>],
) -> Result<(), ProviderConformanceError> {
    if cases.is_empty() {
        return Err(ProviderConformanceError::new(
            "rate provider cases must not be empty",
        ));
    }
    for case in cases {
        let decision = provider
            .check_and_consume(case.request)
            .await
            .map_err(|_| ProviderConformanceError::new("rate provider failed"))?;
        validate_decision(decision, case.request, Some(case.expected))?;
    }
    Ok(())
}

/// Constructs all futures before concurrent polling, then verifies the exact
/// number of requests admitted from a full bucket.
pub async fn assert_rate_provider_concurrency_conformance(
    provider: &dyn RateLimiter,
    case: RateProviderConcurrencyCase<'_>,
) -> Result<(), ProviderConformanceError> {
    let expected_allowed = case.request.policy().limit() / case.request.cost().get();
    if case.attempts as u64 <= expected_allowed {
        return Err(ProviderConformanceError::new(
            "concurrency attempts must exceed the full bucket allowance",
        ));
    }
    if case.attempts > 256 {
        return Err(ProviderConformanceError::new(
            "concurrency attempts exceed the conformance bound",
        ));
    }
    let mut allowed = 0u64;
    for result in poll_concurrently(provider, case)? {
        let decision =
            result.map_err(|_| ProviderConformanceError::new("concurrent rate provider failed"))?;
        validate_decision(decision, case.request, None)?;
        if decision.is_allowed() {
            allowed += 1;
        }
    }
    if allowed != expected_allowed {
        return Err(ProviderConformanceError::new(
            "concurrent consume exceeded or underused capacity",
        ));
    }
    Ok(())
}

/// Verifies that clock reversal does not add tokens and idle expiry releases the entry.
pub async fn assert_rate_provider_time_conformance(
    provider: &dyn RateLimiter,
    control: &dyn RateProviderTimeControl,
    case: RateProviderTimeCase<'_>,
) -> Result<(), ProviderConformanceError> {
    if case.rollback >= case.initial
        || case.before_expiry <= case.initial
        || case.after_primary_expiry <= case.before_expiry
        || case.after_secondary_expiry <= case.after_primary_expiry
    {
        return Err(ProviderConformanceError::new(
            "time fixture ticks are inconsistent",
        ));
    }
    control.set_now(case.initial);
    let allowance = case.primary.policy().limit() / case.primary.cost().get();
    for _ in 0..allowance {
        expect(provider, case.primary, ExpectedRateDecision::Allowed).await?;
    }
    expect(provider, case.primary, ExpectedRateDecision::Rejected).await?;
    control.set_now(case.rollback);
    expect(provider, case.primary, ExpectedRateDecision::Rejected).await?;
    control.set_now(case.before_expiry);
    expect(provider, case.primary, ExpectedRateDecision::Rejected).await?;
    control.set_now(case.after_primary_expiry);
    expect(provider, case.secondary, ExpectedRateDecision::Allowed).await?;
    expect_entry_count(control, 1)?;
    expect_reject_reason(
        provider,
        case.primary,
        RateLimitRejectReason::KeyCapacityExhausted,
    )
    .await?;
    expect_entry_count(control, 1)?;
    control.set_now(case.after_secondary_expiry);
    expect(provider, case.primary, ExpectedRateDecision::Allowed).await?;
    expect_entry_count(control, 1)
}

fn poll_concurrently(
    provider: &dyn RateLimiter,
    case: RateProviderConcurrencyCase<'_>,
) -> Result<Vec<Result<RateLimitDecision, RateLimitProviderError>>, ProviderConformanceError> {
    let simultaneous_poll = Arc::new(Barrier::new(case.attempts));
    let worker_start = Arc::new((Mutex::new(None), Condvar::new()));
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(case.attempts);
        for index in 0..case.attempts {
            let simultaneous_poll = Arc::clone(&simultaneous_poll);
            let worker_gate = Arc::clone(&worker_start);
            let worker = std::thread::Builder::new()
                .name(format!("chigiri-rate-conformance-{index}"))
                .spawn_scoped(scope, move || {
                    wait_for_worker_start(&worker_gate)?;
                    simultaneous_poll.wait();
                    Ok(block_on(provider.check_and_consume(case.request)))
                });
            match worker {
                Ok(worker) => handles.push(worker),
                Err(_) => {
                    release_workers(&worker_start, false)?;
                    for handle in handles {
                        let _ = handle.join().map_err(|_| {
                            ProviderConformanceError::new("conformance worker panicked")
                        })??;
                    }
                    return Err(ProviderConformanceError::new(
                        "conformance worker spawn failed",
                    ));
                }
            }
        }
        release_workers(&worker_start, true)?;
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| ProviderConformanceError::new("conformance worker panicked"))
                    .and_then(|result| result)
            })
            .collect()
    })
}

fn wait_for_worker_start(
    worker_start: &(Mutex<Option<bool>>, Condvar),
) -> Result<(), ProviderConformanceError> {
    let (state, changed) = worker_start;
    let mut state = state
        .lock()
        .map_err(|_| ProviderConformanceError::new("conformance start gate was poisoned"))?;
    while state.is_none() {
        state = changed
            .wait(state)
            .map_err(|_| ProviderConformanceError::new("conformance start gate was poisoned"))?;
    }
    if *state == Some(true) {
        Ok(())
    } else {
        Err(ProviderConformanceError::new(
            "conformance worker spawn failed",
        ))
    }
}

fn release_workers(
    worker_start: &(Mutex<Option<bool>>, Condvar),
    run: bool,
) -> Result<(), ProviderConformanceError> {
    let (state, changed) = worker_start;
    *state
        .lock()
        .map_err(|_| ProviderConformanceError::new("conformance start gate was poisoned"))? =
        Some(run);
    changed.notify_all();
    Ok(())
}

struct ThreadWake(std::thread::Thread);

impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on(mut future: RateLimitFuture<'_>) -> Result<RateLimitDecision, RateLimitProviderError> {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::park(),
        }
    }
}

async fn expect(
    provider: &dyn RateLimiter,
    request: RateLimitRequest<'_>,
    expected: ExpectedRateDecision,
) -> Result<(), ProviderConformanceError> {
    let decision = provider
        .check_and_consume(request)
        .await
        .map_err(|_| ProviderConformanceError::new("time provider failed"))?;
    validate_decision(decision, request, Some(expected))
}

async fn expect_reject_reason(
    provider: &dyn RateLimiter,
    request: RateLimitRequest<'_>,
    expected: RateLimitRejectReason,
) -> Result<(), ProviderConformanceError> {
    let decision = provider
        .check_and_consume(request)
        .await
        .map_err(|_| ProviderConformanceError::new("time provider failed"))?;
    validate_decision(decision, request, Some(ExpectedRateDecision::Rejected))?;
    let reason = decision.snapshot().reject_reason();
    if reason != Some(expected) {
        return Err(ProviderConformanceError::new(
            "time provider rejection reason is inconsistent",
        ));
    }
    Ok(())
}

fn expect_entry_count(
    control: &dyn RateProviderTimeControl,
    expected: usize,
) -> Result<(), ProviderConformanceError> {
    let actual = control
        .entry_count()
        .map_err(|_| ProviderConformanceError::new("entry count failed"))?;
    if actual != expected {
        return Err(ProviderConformanceError::new(
            "expired provider entries were not released",
        ));
    }
    Ok(())
}

fn validate_decision(
    decision: RateLimitDecision,
    request: RateLimitRequest<'_>,
    expected: Option<ExpectedRateDecision>,
) -> Result<(), ProviderConformanceError> {
    let actual = if decision.is_allowed() {
        ExpectedRateDecision::Allowed
    } else {
        ExpectedRateDecision::Rejected
    };
    let snapshot = decision.snapshot();
    if expected.is_some_and(|expected| expected != actual)
        || snapshot.limit() != request.policy().limit()
        || snapshot.remaining() > snapshot.limit()
        || snapshot.revision() != request.policy().revision()
        || (actual == ExpectedRateDecision::Allowed && snapshot.retry_after().is_some())
        || (actual == ExpectedRateDecision::Allowed && snapshot.reject_reason().is_some())
        || (actual == ExpectedRateDecision::Rejected && snapshot.reject_reason().is_none())
        || (actual == ExpectedRateDecision::Rejected
            && snapshot.retry_after().is_none()
            && snapshot.reject_reason() == Some(RateLimitRejectReason::QuotaExceeded))
    {
        return Err(ProviderConformanceError::new(
            "rate decision metadata is inconsistent",
        ));
    }
    Ok(())
}
