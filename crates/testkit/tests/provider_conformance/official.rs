//! Applies the public third-party conformance suite to all four official algorithms.

use std::sync::Arc;
use std::time::Duration;

use chigiri_rate::{
    FixedWindowPolicy, LeakyBucketPolicy, MonotonicTick, OperationCost, QuotaKey, QuotaSubject,
    RateLimitProviderError, RateLimitRequest, RateLimiter, RatePolicy, SlidingWindowPolicy,
    TokenBucketPolicy,
};
use chigiri_rate_memory::{
    Clock, InMemoryFixedWindow, InMemoryLeakyBucket, InMemoryRateLimitConfig,
    InMemorySlidingWindowCounter, InMemoryTokenBucket, ManualClock,
};
use chigiri_testkit::{
    ExpectedRateDecision, RateProviderCase, RateProviderConcurrencyCase, RateProviderTimeCase,
    RateProviderTimeControl, assert_rate_provider_concurrency_conformance,
    assert_rate_provider_conformance, assert_rate_provider_nonblocking_conformance,
    assert_rate_provider_time_conformance,
};

#[path = "official/state.rs"]
mod state;

#[tokio::test]
async fn all_four_official_algorithms_satisfy_the_provider_contract() {
    let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(1)));
    let ttl = Duration::from_secs(60);
    let token_policy = TokenBucketPolicy::new(2, 1, Duration::from_secs(60))
        .unwrap()
        .into();
    let fixed_policy = FixedWindowPolicy::new(2, Duration::from_secs(60))
        .unwrap()
        .into();
    let sliding_policy = SlidingWindowPolicy::new(2, Duration::from_secs(60))
        .unwrap()
        .into();
    let leaky_policy = LeakyBucketPolicy::new(2, 1, Duration::from_secs(60))
        .unwrap()
        .into();

    check_nonblocking(InMemoryTokenBucket::new(), token_policy, b"token-poll");
    check_nonblocking(InMemoryFixedWindow::new(), fixed_policy, b"fixed-poll");
    check_nonblocking(
        InMemorySlidingWindowCounter::new(),
        sliding_policy,
        b"sliding-poll",
    );
    check_nonblocking(InMemoryLeakyBucket::new(), leaky_policy, b"leaky-poll");

    check_sequential(
        &InMemoryTokenBucket::with_clock(config(4, ttl), Arc::clone(&clock)),
        token_policy,
        b"token-sequential",
    )
    .await;
    check_sequential(
        &InMemoryFixedWindow::with_clock(config(4, ttl), Arc::clone(&clock)),
        fixed_policy,
        b"fixed-sequential",
    )
    .await;
    check_sequential(
        &InMemorySlidingWindowCounter::with_clock(config(4, ttl), Arc::clone(&clock)),
        sliding_policy,
        b"sliding-sequential",
    )
    .await;
    check_sequential(
        &InMemoryLeakyBucket::with_clock(config(4, ttl), Arc::clone(&clock)),
        leaky_policy,
        b"leaky-sequential",
    )
    .await;

    check_concurrent(
        &InMemoryTokenBucket::with_clock(config(4, ttl), Arc::clone(&clock)),
        token_policy,
        b"token-concurrent",
    )
    .await;
    check_concurrent(
        &InMemoryFixedWindow::with_clock(config(4, ttl), Arc::clone(&clock)),
        fixed_policy,
        b"fixed-concurrent",
    )
    .await;
    check_concurrent(
        &InMemorySlidingWindowCounter::with_clock(config(4, ttl), Arc::clone(&clock)),
        sliding_policy,
        b"sliding-concurrent",
    )
    .await;
    check_concurrent(
        &InMemoryLeakyBucket::with_clock(config(4, ttl), clock),
        leaky_policy,
        b"leaky-concurrent",
    )
    .await;

    check_token_time(token_policy).await;
    check_fixed_time(fixed_policy).await;
    check_sliding_time(sliding_policy).await;
    check_leaky_time(leaky_policy).await;
}

fn check_nonblocking<P>(provider: P, policy: RatePolicy, label: &[u8])
where
    P: RateLimiter,
{
    let key = quota_key(label);
    let provider: Arc<dyn RateLimiter> = Arc::new(provider);
    assert_rate_provider_nonblocking_conformance(
        provider,
        RateLimitRequest::new(&key, OperationCost::one(), policy),
        Duration::from_millis(100),
    )
    .unwrap();
}

async fn check_sequential(provider: &dyn RateLimiter, policy: RatePolicy, label: &[u8]) {
    let key = quota_key(label);
    let request = RateLimitRequest::new(&key, OperationCost::one(), policy);
    assert_rate_provider_conformance(
        provider,
        &[
            RateProviderCase {
                request,
                expected: ExpectedRateDecision::Allowed,
            },
            RateProviderCase {
                request,
                expected: ExpectedRateDecision::Allowed,
            },
            RateProviderCase {
                request,
                expected: ExpectedRateDecision::Rejected,
            },
        ],
    )
    .await
    .unwrap();
}

async fn check_concurrent(provider: &dyn RateLimiter, policy: RatePolicy, label: &[u8]) {
    let key = quota_key(label);
    assert_rate_provider_concurrency_conformance(
        provider,
        RateProviderConcurrencyCase {
            request: RateLimitRequest::new(&key, OperationCost::one(), policy),
            attempts: 8,
        },
    )
    .await
    .unwrap();
}

fn quota_key(value: &[u8]) -> QuotaKey {
    QuotaKey::new(value.to_vec(), QuotaSubject::Missing, None).unwrap()
}

trait EntryCount {
    fn entry_count(&self) -> Result<usize, RateLimitProviderError>;
}

impl<C: Clock> EntryCount for InMemoryTokenBucket<C> {
    fn entry_count(&self) -> Result<usize, RateLimitProviderError> {
        self.entry_count()
    }
}

impl<C: Clock> EntryCount for InMemoryFixedWindow<C> {
    fn entry_count(&self) -> Result<usize, RateLimitProviderError> {
        self.entry_count()
    }
}

impl<C: Clock> EntryCount for InMemorySlidingWindowCounter<C> {
    fn entry_count(&self) -> Result<usize, RateLimitProviderError> {
        self.entry_count()
    }
}

impl<C: Clock> EntryCount for InMemoryLeakyBucket<C> {
    fn entry_count(&self) -> Result<usize, RateLimitProviderError> {
        self.entry_count()
    }
}

struct TimeControl<'a, P> {
    clock: Arc<ManualClock>,
    provider: &'a P,
}

impl<P: EntryCount + Sync> RateProviderTimeControl for TimeControl<'_, P> {
    fn set_now(&self, tick: MonotonicTick) {
        self.clock.set(tick);
    }

    fn entry_count(&self) -> Result<usize, RateLimitProviderError> {
        self.provider.entry_count()
    }
}

async fn check_time<P>(provider: &P, clock: Arc<ManualClock>, policy: RatePolicy, label: &[u8])
where
    P: EntryCount + RateLimiter + Sync,
{
    let primary = quota_key(label);
    let mut secondary_label = label.to_vec();
    secondary_label.extend_from_slice(b"-secondary");
    let secondary = quota_key(&secondary_label);
    let control = TimeControl { clock, provider };
    assert_rate_provider_time_conformance(
        provider,
        &control,
        RateProviderTimeCase {
            primary: RateLimitRequest::new(&primary, OperationCost::one(), policy),
            secondary: RateLimitRequest::new(&secondary, OperationCost::one(), policy),
            initial: MonotonicTick::from_nanos(100),
            rollback: MonotonicTick::from_nanos(10),
            before_expiry: MonotonicTick::from_nanos(149),
            after_primary_expiry: MonotonicTick::from_nanos(200),
            after_secondary_expiry: MonotonicTick::from_nanos(250),
        },
    )
    .await
    .unwrap();
}

async fn check_token_time(policy: RatePolicy) {
    let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(100)));
    let provider =
        InMemoryTokenBucket::with_clock(config(1, Duration::from_nanos(50)), Arc::clone(&clock));
    check_time(&provider, clock, policy, b"token-time").await;
}

async fn check_fixed_time(policy: RatePolicy) {
    let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(100)));
    let provider =
        InMemoryFixedWindow::with_clock(config(1, Duration::from_nanos(50)), Arc::clone(&clock));
    check_time(&provider, clock, policy, b"fixed-time").await;
}

async fn check_sliding_time(policy: RatePolicy) {
    let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(100)));
    let provider = InMemorySlidingWindowCounter::with_clock(
        config(1, Duration::from_nanos(50)),
        Arc::clone(&clock),
    );
    check_time(&provider, clock, policy, b"sliding-time").await;
}

async fn check_leaky_time(policy: RatePolicy) {
    let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(100)));
    let provider =
        InMemoryLeakyBucket::with_clock(config(1, Duration::from_nanos(50)), Arc::clone(&clock));
    check_time(&provider, clock, policy, b"leaky-time").await;
}

fn config(max_keys: usize, idle_ttl: Duration) -> InMemoryRateLimitConfig {
    InMemoryRateLimitConfig::new(max_keys, idle_ttl).unwrap()
}
