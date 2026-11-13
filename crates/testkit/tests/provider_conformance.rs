use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chigiri_rate::{
    MonotonicTick, OperationCost, QuotaKey, QuotaSubject, RateLimitDecision, RateLimitFuture,
    RateLimitRejectReason, RateLimitRequest, RateLimiter, RatePolicy, TokenBucketPolicy,
};
use chigiri_testkit::{
    ExpectedRateDecision, PendingDropProbe, RateProviderCase, RateProviderConcurrencyCase,
    RateProviderStateCase, assert_provider_future_is_droppable,
    assert_rate_provider_concurrency_conformance, assert_rate_provider_conformance,
    assert_rate_provider_state_conformance,
};
#[path = "provider_conformance/blocking.rs"]
mod blocking;
#[path = "provider_conformance/concurrency.rs"]
mod concurrency;
#[path = "provider_conformance/official.rs"]
mod official;
#[path = "provider_conformance/operation.rs"]
mod operation;
#[path = "provider_conformance/time.rs"]
mod time;

struct ThirdPartyLimiter(AtomicU64);

impl RateLimiter for ThirdPartyLimiter {
    fn check_and_consume<'a>(&'a self, request: RateLimitRequest<'a>) -> RateLimitFuture<'a> {
        Box::pin(async move {
            let capacity = request.policy().limit();
            let consumed = self
                .0
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |previous| {
                    previous
                        .checked_add(request.cost().get())
                        .filter(|candidate| *candidate <= capacity)
                });
            if let Ok(previous) = consumed {
                Ok(RateLimitDecision::allowed(
                    request,
                    capacity - previous - request.cost().get(),
                    MonotonicTick::from_nanos(1),
                )
                .unwrap())
            } else {
                Ok(RateLimitDecision::rejected(
                    request,
                    0,
                    MonotonicTick::from_nanos(1),
                    Some(Duration::from_secs(1)),
                    RateLimitRejectReason::QuotaExceeded,
                )
                .unwrap())
            }
        })
    }
}

#[tokio::test]
async fn third_party_rate_provider_runs_public_contract() {
    let key = QuotaKey::new(b"test".to_vec(), QuotaSubject::Missing, None).unwrap();
    let initial_policy = policy(1);
    let request = RateLimitRequest::new(&key, OperationCost::one(), initial_policy);
    let provider = ThirdPartyLimiter(AtomicU64::new(0));
    assert_rate_provider_conformance(
        &provider,
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

    let concurrent = ThirdPartyLimiter(AtomicU64::new(0));
    let concurrent_policy = policy(8);
    let concurrent_request = RateLimitRequest::new(&key, OperationCost::one(), concurrent_policy);
    assert_rate_provider_concurrency_conformance(
        &concurrent,
        RateProviderConcurrencyCase {
            request: concurrent_request,
            attempts: 16,
        },
    )
    .await
    .unwrap();
}

struct PendingLimiter(Mutex<Option<PendingDropProbe>>);

impl RateLimiter for PendingLimiter {
    fn check_and_consume<'a>(&'a self, _: RateLimitRequest<'a>) -> RateLimitFuture<'a> {
        let probe = self.0.lock().unwrap().take().unwrap();
        Box::pin(async move {
            probe.await;
            unreachable!("The pending probe is not complete.")
        })
    }
}

#[test]
fn third_party_provider_future_can_drop_directly_at_timeout() {
    let key = QuotaKey::new(b"pending".to_vec(), QuotaSubject::Missing, None).unwrap();
    let policy = policy(1);
    let request = RateLimitRequest::new(&key, OperationCost::one(), policy);
    let (probe, state) = PendingDropProbe::new();
    let provider = PendingLimiter(Mutex::new(Some(probe)));
    assert_provider_future_is_droppable(provider.check_and_consume(request), &state).unwrap();
}

struct MalformedLimiter;

impl RateLimiter for MalformedLimiter {
    fn check_and_consume<'a>(&'a self, request: RateLimitRequest<'a>) -> RateLimitFuture<'a> {
        Box::pin(async move {
            let wrong_policy = policy(request.policy().limit() + 1);
            let wrong_request = RateLimitRequest::new(request.key(), request.cost(), wrong_policy);
            Ok(RateLimitDecision::allowed(wrong_request, 0, MonotonicTick::from_nanos(0)).unwrap())
        })
    }
}

#[tokio::test]
async fn limit_14_rejects_inconsistent_third_party_provider_decisions() {
    let key = QuotaKey::new(b"malformed".to_vec(), QuotaSubject::Missing, None).unwrap();
    let policy = policy(1);
    let request = RateLimitRequest::new(&key, OperationCost::one(), policy);
    assert!(
        assert_rate_provider_conformance(
            &MalformedLimiter,
            &[RateProviderCase {
                request,
                expected: ExpectedRateDecision::Allowed,
            }],
        )
        .await
        .is_err()
    );
}

struct MutatingRejectLimiter(AtomicU64);

impl RateLimiter for MutatingRejectLimiter {
    fn check_and_consume<'a>(&'a self, request: RateLimitRequest<'a>) -> RateLimitFuture<'a> {
        Box::pin(async move {
            let previous = self.0.fetch_add(request.cost().get(), Ordering::SeqCst);
            let policy = request.policy();
            if previous == 0 {
                Ok(RateLimitDecision::allowed(
                    request,
                    policy.limit() - request.cost().get(),
                    MonotonicTick::from_nanos(1),
                )
                .unwrap())
            } else {
                Ok(RateLimitDecision::rejected(
                    request,
                    0,
                    MonotonicTick::from_nanos(1),
                    None,
                    RateLimitRejectReason::PolicyMismatch,
                )
                .unwrap())
            }
        })
    }
}

#[tokio::test]
async fn rspi_08_state_suite_does_not_consume_quota_when_the_provider_rejects() {
    let key = QuotaKey::new(b"state".to_vec(), QuotaSubject::Missing, None).unwrap();
    let initial = policy(2);
    let changed = policy(3);
    let result = assert_rate_provider_state_conformance(
        &MutatingRejectLimiter(AtomicU64::new(0)),
        RateProviderStateCase {
            initial: RateLimitRequest::new(&key, OperationCost::one(), initial),
            rejected: RateLimitRequest::new(&key, OperationCost::one(), changed),
            follow_up: RateLimitRequest::new(&key, OperationCost::one(), initial),
            reason: RateLimitRejectReason::PolicyMismatch,
        },
    )
    .await;
    assert!(result.is_err());
}

fn policy(capacity: u64) -> RatePolicy {
    TokenBucketPolicy::new(capacity, 1, Duration::from_secs(1))
        .unwrap()
        .into()
}
