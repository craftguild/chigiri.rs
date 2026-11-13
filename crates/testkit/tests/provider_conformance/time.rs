//! Test clock rollback, idle expiry, and bounded key capacity.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chigiri_rate::{
    MonotonicTick, OperationCost, QuotaKey, QuotaSubject, RateLimitDecision, RateLimitFuture,
    RateLimitProviderError, RateLimitRejectReason, RateLimitRequest, RateLimiter,
};
use chigiri_testkit::{
    RateProviderTimeCase, RateProviderTimeControl, assert_rate_provider_time_conformance,
};

use super::policy;

struct TimeControlledLimiter {
    now: AtomicU64,
    idle_expiry: u64,
    max_keys: Option<usize>,
    regress_last_seen: bool,
    buckets: Mutex<BTreeMap<QuotaKey, (u64, MonotonicTick)>>,
}

impl TimeControlledLimiter {
    fn new(idle_expiry: u64, max_keys: Option<usize>, regress_last_seen: bool) -> Self {
        Self {
            now: AtomicU64::new(0),
            idle_expiry,
            max_keys,
            regress_last_seen,
            buckets: Mutex::new(BTreeMap::new()),
        }
    }
}

impl RateLimiter for TimeControlledLimiter {
    fn check_and_consume<'a>(&'a self, request: RateLimitRequest<'a>) -> RateLimitFuture<'a> {
        Box::pin(async move {
            let now = MonotonicTick::from_nanos(self.now.load(Ordering::SeqCst));
            let mut buckets = self.buckets.lock().unwrap();
            buckets.retain(|_, (_, last_seen)| now.saturating_sub(*last_seen) < self.idle_expiry);
            if !buckets.contains_key(request.key())
                && self
                    .max_keys
                    .is_some_and(|max_keys| buckets.len() >= max_keys)
            {
                return Ok(capacity_rejection(request, now));
            }
            let remaining = buckets
                .entry(request.key().clone())
                .or_insert((request.policy().limit(), now));
            let effective_now = now.max(remaining.1);
            remaining.1 = if self.regress_last_seen {
                now
            } else {
                effective_now
            };
            let allowed = remaining.0 >= request.cost().get();
            if allowed {
                remaining.0 -= request.cost().get();
            }
            let decision = if allowed {
                RateLimitDecision::allowed(request, remaining.0, effective_now)
            } else {
                RateLimitDecision::rejected(
                    request,
                    remaining.0,
                    effective_now,
                    Some(Duration::from_nanos(1)),
                    RateLimitRejectReason::QuotaExceeded,
                )
            }
            .unwrap();
            Ok(decision)
        })
    }
}

impl RateProviderTimeControl for TimeControlledLimiter {
    fn set_now(&self, tick: MonotonicTick) {
        self.now.store(tick.as_nanos(), Ordering::SeqCst);
    }

    fn entry_count(&self) -> Result<usize, RateLimitProviderError> {
        Ok(self.buckets.lock().unwrap().len())
    }
}

#[tokio::test]
async fn third_party_rate_provider_protects_rollback_and_bounded_expiry_contracts() {
    let primary_key = QuotaKey::new(b"primary".to_vec(), QuotaSubject::Missing, None).unwrap();
    let secondary_key = QuotaKey::new(b"secondary".to_vec(), QuotaSubject::Missing, None).unwrap();
    let policy = policy(2);
    let case = RateProviderTimeCase {
        primary: RateLimitRequest::new(&primary_key, OperationCost::one(), policy),
        secondary: RateLimitRequest::new(&secondary_key, OperationCost::one(), policy),
        initial: MonotonicTick::from_nanos(100),
        rollback: MonotonicTick::from_nanos(10),
        before_expiry: MonotonicTick::from_nanos(149),
        after_primary_expiry: MonotonicTick::from_nanos(200),
        after_secondary_expiry: MonotonicTick::from_nanos(250),
    };
    let conformant = TimeControlledLimiter::new(50, Some(1), false);
    assert_rate_provider_time_conformance(&conformant, &conformant, case)
        .await
        .unwrap();
    let rollback = TimeControlledLimiter::new(50, Some(1), true);
    assert!(
        assert_rate_provider_time_conformance(&rollback, &rollback, case)
            .await
            .is_err()
    );
    let unbounded = TimeControlledLimiter::new(50, None, false);
    assert!(
        assert_rate_provider_time_conformance(&unbounded, &unbounded, case)
            .await
            .is_err()
    );
}

fn capacity_rejection(request: RateLimitRequest<'_>, now: MonotonicTick) -> RateLimitDecision {
    RateLimitDecision::rejected(
        request,
        0,
        now,
        None,
        RateLimitRejectReason::KeyCapacityExhausted,
    )
    .unwrap()
}
