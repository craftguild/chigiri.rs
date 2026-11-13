use std::sync::Arc;
use std::time::Duration;

use chigiri_rate::{
    FixedWindowPolicy, LeakyBucketPolicy, MonotonicTick, OperationCost, QuotaKey, QuotaSubject,
    RateLimitDecision, RateLimitFuture, RateLimitRejectReason, RateLimitRequest, RateLimiter,
    RatePolicy, SlidingWindowPolicy, TokenBucketPolicy,
};
use chigiri_rate_memory::{
    InMemoryFixedWindow, InMemoryLeakyBucket, InMemoryRateLimitConfig,
    InMemorySlidingWindowCounter, InMemoryTokenBucket, ManualClock,
};

#[test]
fn rmem_10_cancel_capacity_mismatch_and_overcost_do_not_change_state() {
    let manual = clock();
    let provider = InMemoryTokenBucket::with_clock(config(), Arc::clone(&manual));
    exercise_rejections(provider, manual, token_policy(2), token_policy(3));
    let manual = clock();
    let provider = InMemoryFixedWindow::with_clock(config(), Arc::clone(&manual));
    exercise_rejections(provider, manual, fixed_policy(2), fixed_policy(3));
    let manual = clock();
    let provider = InMemorySlidingWindowCounter::with_clock(config(), Arc::clone(&manual));
    exercise_rejections(provider, manual, sliding_policy(2), sliding_policy(3));
    let manual = clock();
    let provider = InMemoryLeakyBucket::with_clock(config(), Arc::clone(&manual));
    exercise_rejections(provider, manual, leaky_policy(2), leaky_policy(3));
}

#[test]
fn rmem_11_all_four_providers_handle_clock_rollback_and_large_forward_jumps() {
    let manual = clock();
    let provider = InMemoryTokenBucket::with_clock(config(), Arc::clone(&manual));
    exercise_clock(provider, manual, token_policy(2));
    let manual = clock();
    let provider = InMemoryFixedWindow::with_clock(config(), Arc::clone(&manual));
    exercise_clock(provider, manual, fixed_policy(2));
    let manual = clock();
    let provider = InMemorySlidingWindowCounter::with_clock(config(), Arc::clone(&manual));
    exercise_clock(provider, manual, sliding_policy(2));
    let manual = clock();
    let provider = InMemoryLeakyBucket::with_clock(config(), Arc::clone(&manual));
    exercise_clock(provider, manual, leaky_policy(2));
}

fn exercise_rejections(
    provider: impl RateLimiter,
    manual: Arc<ManualClock>,
    policy: RatePolicy,
    changed: RatePolicy,
) {
    let cancelled = key(b"cancelled");
    drop(provider.check_and_consume(request(&cancelled, 1, policy)));

    let oversized = key(b"oversized");
    assert_reason(
        decide(&provider, request(&oversized, 3, policy)),
        RateLimitRejectReason::DynamicCostExceedsLimit,
    );
    let primary = key(b"primary");
    assert_allowed(decide(&provider, request(&primary, 1, policy)));
    assert_reason(
        decide(&provider, request(&primary, 1, changed)),
        RateLimitRejectReason::PolicyMismatch,
    );
    let secondary = key(b"secondary");
    assert_reason(
        decide(&provider, request(&secondary, 1, policy)),
        RateLimitRejectReason::KeyCapacityExhausted,
    );
    assert_allowed(decide(&provider, request(&primary, 1, policy)));
    assert_reason(
        decide(&provider, request(&primary, 1, policy)),
        RateLimitRejectReason::QuotaExceeded,
    );
    manual.set(MonotonicTick::from_nanos(9));
    assert_reason(
        decide(&provider, request(&primary, 3, policy)),
        RateLimitRejectReason::DynamicCostExceedsLimit,
    );
    assert_reason(
        decide(&provider, request(&primary, 1, changed)),
        RateLimitRejectReason::PolicyMismatch,
    );
    assert_reason(
        decide(&provider, request(&secondary, 1, policy)),
        RateLimitRejectReason::KeyCapacityExhausted,
    );
    manual.set(MonotonicTick::from_nanos(11));
    assert_allowed(decide(&provider, request(&secondary, 1, policy)));
}

fn exercise_clock<P>(provider: P, manual: Arc<ManualClock>, policy: RatePolicy)
where
    P: RateLimiter,
{
    let subject = key(b"clock");
    manual.set(MonotonicTick::from_nanos(10));
    assert_allowed(decide(&provider, request(&subject, 2, policy)));
    assert_reason(
        decide(&provider, request(&subject, 1, policy)),
        RateLimitRejectReason::QuotaExceeded,
    );
    manual.set(MonotonicTick::from_nanos(1));
    assert_reason(
        decide(&provider, request(&subject, 1, policy)),
        RateLimitRejectReason::QuotaExceeded,
    );
    manual.set(MonotonicTick::from_nanos(u64::MAX));
    assert_allowed(decide(&provider, request(&subject, 1, policy)));
}

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(MonotonicTick::from_nanos(0)))
}

fn config() -> InMemoryRateLimitConfig {
    InMemoryRateLimitConfig::new(1, Duration::from_nanos(10)).unwrap()
}

fn token_policy(limit: u64) -> RatePolicy {
    TokenBucketPolicy::new(limit, limit, Duration::from_nanos(10))
        .unwrap()
        .into()
}

fn fixed_policy(limit: u64) -> RatePolicy {
    FixedWindowPolicy::new(limit, Duration::from_nanos(10))
        .unwrap()
        .into()
}

fn sliding_policy(limit: u64) -> RatePolicy {
    SlidingWindowPolicy::new(limit, Duration::from_nanos(10))
        .unwrap()
        .into()
}

fn leaky_policy(limit: u64) -> RatePolicy {
    LeakyBucketPolicy::new(limit, limit, Duration::from_nanos(10))
        .unwrap()
        .into()
}

fn key(value: &[u8]) -> QuotaKey {
    QuotaKey::new(
        b"state".to_vec(),
        QuotaSubject::value(value.to_vec()).unwrap(),
        None,
    )
    .unwrap()
}

fn request(key: &QuotaKey, cost: u64, policy: RatePolicy) -> RateLimitRequest<'_> {
    RateLimitRequest::new(key, OperationCost::new(cost).unwrap(), policy)
}

fn decide(provider: &impl RateLimiter, request: RateLimitRequest<'_>) -> RateLimitDecision {
    poll(provider.check_and_consume(request)).unwrap()
}

fn poll(
    future: RateLimitFuture<'_>,
) -> Result<RateLimitDecision, chigiri_rate::RateLimitProviderError> {
    let waker = std::task::Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = future;
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => return value,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

struct ThreadWake(std::thread::Thread);

impl std::task::Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

fn assert_allowed(decision: RateLimitDecision) {
    assert!(decision.is_allowed());
}

fn assert_reason(decision: RateLimitDecision, expected: RateLimitRejectReason) {
    assert!(!decision.is_allowed(), "expected a rejection decision");
    assert_eq!(decision.snapshot().reject_reason(), Some(expected));
}
