//! Checks single- and multi-poll atomicity under real thread contention.

use std::future::Future;
use std::pin::Pin;
use std::sync::Barrier;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_rate::{
    MonotonicTick, OperationCost, QuotaKey, QuotaSubject, RateLimitDecision, RateLimitFuture,
    RateLimitProviderError, RateLimitRejectReason, RateLimitRequest, RateLimiter,
};
use chigiri_testkit::{RateProviderConcurrencyCase, assert_rate_provider_concurrency_conformance};

use super::policy;

struct ReadyRacyLimiter {
    consumed: AtomicU64,
    polls: Barrier,
}

impl RateLimiter for ReadyRacyLimiter {
    fn check_and_consume<'a>(&'a self, request: RateLimitRequest<'a>) -> RateLimitFuture<'a> {
        Box::pin(ReadyRacyDecision {
            consumed: &self.consumed,
            polls: &self.polls,
            request,
        })
    }
}

struct ReadyRacyDecision<'a> {
    consumed: &'a AtomicU64,
    polls: &'a Barrier,
    request: RateLimitRequest<'a>,
}

impl Future for ReadyRacyDecision<'_> {
    type Output = Result<RateLimitDecision, RateLimitProviderError>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        let observed = self.consumed.load(Ordering::SeqCst);
        self.polls.wait();
        let cost = self.request.cost().get();
        let allowed = observed + cost <= self.request.policy().limit();
        if allowed {
            self.consumed.store(observed + cost, Ordering::SeqCst);
        }
        Poll::Ready(Ok(decision(allowed, self.request, observed)))
    }
}

#[tokio::test]
async fn concurrent_suite_rejects_non_atomic_providers_including_single_poll() {
    let key = QuotaKey::new(b"racy".to_vec(), QuotaSubject::Missing, None).unwrap();
    let request = RateLimitRequest::new(&key, OperationCost::one(), policy(2));
    assert!(
        assert_rate_provider_concurrency_conformance(
            &ReadyRacyLimiter {
                consumed: AtomicU64::new(0),
                polls: Barrier::new(4),
            },
            RateProviderConcurrencyCase {
                request,
                attempts: 4,
            },
        )
        .await
        .is_err()
    );
    let bounded = assert_rate_provider_concurrency_conformance(
        &ReadyRacyLimiter {
            consumed: AtomicU64::new(0),
            polls: Barrier::new(1),
        },
        RateProviderConcurrencyCase {
            request,
            attempts: 257,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(
        bounded.to_string(),
        "concurrency attempts exceed the conformance bound"
    );
}

fn decision(allowed: bool, request: RateLimitRequest<'_>, observed: u64) -> RateLimitDecision {
    let capacity = request.policy().limit();
    let cost = request.cost().get();
    let remaining = capacity.saturating_sub(observed + cost);
    if allowed {
        RateLimitDecision::allowed(request, remaining, MonotonicTick::from_nanos(0)).unwrap()
    } else {
        RateLimitDecision::rejected(
            request,
            remaining,
            MonotonicTick::from_nanos(0),
            Some(Duration::from_nanos(1)),
            RateLimitRejectReason::QuotaExceeded,
        )
        .unwrap()
    }
}
