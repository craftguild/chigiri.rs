use std::time::Duration;

use chigiri_rate::{
    OperationCost, QuotaKey, QuotaSubject, RateLimitRequest, RatePolicy, TokenBucketPolicy,
};
use chigiri_rate_provider_minimal::AtomicCounterLimiter;
use chigiri_testkit::{
    ExpectedRateDecision, RateProviderCase, RateProviderConcurrencyCase,
    assert_rate_provider_concurrency_conformance, assert_rate_provider_conformance,
};

#[tokio::test]
async fn minimal_provider_satisfies_public_conformance_suites() {
    let key = QuotaKey::new(b"third-party".to_vec(), QuotaSubject::Missing, None).unwrap();
    let single_policy = policy(1);
    let request = RateLimitRequest::new(&key, OperationCost::one(), single_policy);
    assert_rate_provider_conformance(
        &AtomicCounterLimiter::new(),
        &[
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

    let concurrent_policy = policy(8);
    assert_rate_provider_concurrency_conformance(
        &AtomicCounterLimiter::new(),
        RateProviderConcurrencyCase {
            request: RateLimitRequest::new(&key, OperationCost::one(), concurrent_policy),
            attempts: 32,
        },
    )
    .await
    .unwrap();
}

fn policy(capacity: u64) -> RatePolicy {
    TokenBucketPolicy::new(capacity, 1, Duration::from_secs(1))
        .unwrap()
        .into()
}
