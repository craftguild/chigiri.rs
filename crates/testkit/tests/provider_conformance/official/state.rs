//! Applies the shared closed rejection-state suite to all four official providers.

use std::time::Duration;

use chigiri_rate::{
    FixedWindowPolicy, LeakyBucketPolicy, OperationCost, QuotaKey, QuotaSubject,
    RateLimitRejectReason, RateLimitRequest, RateLimiter, RatePolicy, SlidingWindowPolicy,
    TokenBucketPolicy,
};
use chigiri_rate_memory::{
    InMemoryFixedWindow, InMemoryLeakyBucket, InMemoryRateLimitConfig,
    InMemorySlidingWindowCounter, InMemoryTokenBucket,
};
use chigiri_testkit::{RateProviderStateCase, assert_rate_provider_state_conformance};

#[tokio::test]
async fn rspi_08_all_official_providers_preserve_closed_rejection_accounting() {
    check(
        || InMemoryTokenBucket::with_config(config()),
        token_policy(2),
        token_policy(3),
        b"token",
    )
    .await;
    check(
        || InMemoryFixedWindow::with_config(config()),
        fixed_policy(2),
        fixed_policy(3),
        b"fixed",
    )
    .await;
    check(
        || InMemorySlidingWindowCounter::with_config(config()),
        sliding_policy(2),
        sliding_policy(3),
        b"sliding",
    )
    .await;
    check(
        || InMemoryLeakyBucket::with_config(config()),
        leaky_policy(2),
        leaky_policy(3),
        b"leaky",
    )
    .await;
}

async fn check<P>(factory: impl Fn() -> P, policy: RatePolicy, changed: RatePolicy, label: &[u8])
where
    P: RateLimiter,
{
    let primary = key(label, b"primary");
    let secondary = key(label, b"secondary");
    let provider = factory();
    check_case(
        &provider,
        request(&primary, 1, policy),
        request(&primary, 1, changed),
        request(&primary, 1, policy),
        RateLimitRejectReason::PolicyMismatch,
    )
    .await;

    let provider = factory();
    check_case(
        &provider,
        request(&primary, 1, policy),
        request(&primary, 3, policy),
        request(&primary, 1, policy),
        RateLimitRejectReason::DynamicCostExceedsLimit,
    )
    .await;

    let provider = factory();
    check_case(
        &provider,
        request(&primary, 1, policy),
        request(&secondary, 1, policy),
        request(&primary, 1, policy),
        RateLimitRejectReason::KeyCapacityExhausted,
    )
    .await;
}

async fn check_case(
    provider: &dyn RateLimiter,
    initial: RateLimitRequest<'_>,
    rejected: RateLimitRequest<'_>,
    follow_up: RateLimitRequest<'_>,
    reason: RateLimitRejectReason,
) {
    assert_rate_provider_state_conformance(
        provider,
        RateProviderStateCase {
            initial,
            rejected,
            follow_up,
            reason,
        },
    )
    .await
    .unwrap();
}

fn request(key: &QuotaKey, cost: u64, policy: RatePolicy) -> RateLimitRequest<'_> {
    RateLimitRequest::new(key, OperationCost::new(cost).unwrap(), policy)
}

fn config() -> InMemoryRateLimitConfig {
    InMemoryRateLimitConfig::new(1, Duration::from_secs(300)).unwrap()
}

fn key(prefix: &[u8], suffix: &[u8]) -> QuotaKey {
    QuotaKey::new(
        prefix.to_vec(),
        QuotaSubject::value(suffix.to_vec()).unwrap(),
        None,
    )
    .unwrap()
}

fn token_policy(limit: u64) -> RatePolicy {
    TokenBucketPolicy::new(limit, 1, Duration::from_secs(60))
        .unwrap()
        .into()
}

fn fixed_policy(limit: u64) -> RatePolicy {
    FixedWindowPolicy::new(limit, Duration::from_secs(60))
        .unwrap()
        .into()
}

fn sliding_policy(limit: u64) -> RatePolicy {
    SlidingWindowPolicy::new(limit, Duration::from_secs(60))
        .unwrap()
        .into()
}

fn leaky_policy(limit: u64) -> RatePolicy {
    LeakyBucketPolicy::new(limit, 1, Duration::from_secs(60))
        .unwrap()
        .into()
}
