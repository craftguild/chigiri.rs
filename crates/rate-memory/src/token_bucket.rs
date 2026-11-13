//! Token-bucket rate limiting with whole-interval refills.

use std::time::Duration;

use chigiri_rate::RateLimitFuture;
use std::sync::TryLockError;

use crate::{AsyncMutex, Clock, InMemoryRateLimitConfig, MonotonicTick, SystemClock};

use super::store::{BoundedStore, LastSeen, observed_now};
use super::{
    BucketLockBusy, BucketLockPoisoned, RateLimitDecision, RateLimitProviderError,
    RateLimitRejectReason, RateLimitRequest, RateLimiter, RatePolicy, TokenBucketPolicy,
};

#[derive(Clone, Copy)]
struct Bucket {
    tokens: u64,
    last_refill: MonotonicTick,
    last_seen: MonotonicTick,
    policy: TokenBucketPolicy,
}

impl LastSeen for Bucket {
    fn last_seen(&self) -> MonotonicTick {
        self.last_seen
    }
}

/// A bounded token-bucket provider shared only within the process.
pub struct InMemoryTokenBucket<C = crate::SystemClock> {
    clock: C,
    buckets: AsyncMutex<BoundedStore<Bucket>>,
}

impl<C> InMemoryTokenBucket<C>
where
    C: Clock,
{
    pub fn with_clock(config: InMemoryRateLimitConfig, clock: C) -> Self {
        Self {
            clock,
            buckets: AsyncMutex::new(BoundedStore::new(
                config.max_keys(),
                config.idle_ttl_nanos(),
            )),
        }
    }

    async fn decide(
        &self,
        request: RateLimitRequest<'_>,
    ) -> Result<RateLimitDecision, RateLimitProviderError> {
        let RatePolicy::TokenBucket(policy) = request.policy() else {
            return rejection(request, RateLimitRejectReason::PolicyMismatch, None, 0);
        };
        let now = self.clock.now();
        if request.cost().get() > policy.capacity() {
            return rejection(
                request,
                RateLimitRejectReason::DynamicCostExceedsLimit,
                None,
                0,
            );
        }
        let mut buckets = crate::lock_store(&self.buckets).await?;
        buckets.maintain_one(now, Bucket::last_seen);
        let key = request.key().clone();
        let bucket = match buckets.get_mut(&key) {
            Some(bucket) if bucket.policy != policy => {
                return rejection(request, RateLimitRejectReason::PolicyMismatch, None, 0);
            }
            Some(bucket) => bucket,
            None => {
                let state = Bucket {
                    tokens: policy.capacity(),
                    last_refill: now,
                    last_seen: now,
                    policy,
                };
                match buckets.insert(key, state) {
                    Ok(bucket) => bucket,
                    Err(()) => {
                        return rejection(
                            request,
                            RateLimitRejectReason::KeyCapacityExhausted,
                            None,
                            0,
                        );
                    }
                }
            }
        };
        let effective_now = observed_now(now, bucket.last_seen);
        let mut next_bucket = *bucket;
        refill(&mut next_bucket, effective_now);
        next_bucket.last_seen = effective_now;
        let cost = request.cost().get();
        if cost <= next_bucket.tokens {
            next_bucket.tokens -= cost;
            let until_reset = match until_capacity(next_bucket, effective_now) {
                Some(value) => value,
                None => {
                    return rejection(
                        request,
                        RateLimitRejectReason::ArithmeticOverflow,
                        None,
                        next_bucket.tokens,
                    );
                }
            };
            let reset = effective_now.saturating_add(until_reset);
            let decision = RateLimitDecision::allowed(request, next_bucket.tokens, reset)
                .map_err(RateLimitProviderError::new)?;
            *bucket = next_bucket;
            Ok(decision)
        } else {
            let needed = cost - next_bucket.tokens;
            let intervals = needed.div_ceil(policy.tokens_per_interval());
            let Some(retry) = until_intervals(next_bucket, effective_now, intervals) else {
                return rejection(
                    request,
                    RateLimitRejectReason::ArithmeticOverflow,
                    None,
                    next_bucket.tokens,
                );
            };
            let reset = effective_now
                .saturating_add(until_capacity(next_bucket, effective_now).unwrap_or(u64::MAX));
            let decision = RateLimitDecision::rejected(
                request,
                next_bucket.tokens,
                reset,
                Some(Duration::from_nanos(retry)),
                RateLimitRejectReason::QuotaExceeded,
            )
            .map_err(RateLimitProviderError::new)?;
            bucket.last_seen = effective_now;
            Ok(decision)
        }
    }

    pub fn entry_count(&self) -> Result<usize, RateLimitProviderError> {
        match self.buckets.try_lock() {
            Ok(buckets) => Ok(buckets.len()),
            Err(TryLockError::Poisoned(_)) => Err(RateLimitProviderError::new(BucketLockPoisoned)),
            Err(TryLockError::WouldBlock) => Err(RateLimitProviderError::new(BucketLockBusy)),
        }
    }
}

impl InMemoryTokenBucket<SystemClock> {
    pub fn new() -> Self {
        Self::with_config(InMemoryRateLimitConfig::default())
    }

    pub fn with_config(config: InMemoryRateLimitConfig) -> Self {
        Self::with_clock(config, SystemClock::new())
    }
}

impl Default for InMemoryTokenBucket<SystemClock> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Clock> RateLimiter for InMemoryTokenBucket<C> {
    fn check_and_consume<'a>(&'a self, request: RateLimitRequest<'a>) -> RateLimitFuture<'a> {
        Box::pin(self.decide(request))
    }
}

fn refill(bucket: &mut Bucket, now: MonotonicTick) {
    let elapsed = now.saturating_sub(bucket.last_refill);
    let intervals = elapsed / bucket.policy.interval_nanos();
    if intervals == 0 {
        return;
    }
    let added = u128::from(intervals) * u128::from(bucket.policy.tokens_per_interval());
    bucket.tokens = u128::from(bucket.tokens)
        .saturating_add(added)
        .min(u128::from(bucket.policy.capacity())) as u64;
    bucket.last_refill = bucket
        .last_refill
        .saturating_add(intervals.saturating_mul(bucket.policy.interval_nanos()));
}

fn until_capacity(bucket: Bucket, now: MonotonicTick) -> Option<u64> {
    let missing = bucket.policy.capacity().saturating_sub(bucket.tokens);
    if missing == 0 {
        return Some(0);
    }
    until_intervals(
        bucket,
        now,
        missing.div_ceil(bucket.policy.tokens_per_interval()),
    )
}

fn until_intervals(bucket: Bucket, now: MonotonicTick, intervals: u64) -> Option<u64> {
    if intervals == 0 {
        return Some(0);
    }
    let elapsed = now.saturating_sub(bucket.last_refill);
    let until_next = bucket
        .policy
        .interval_nanos()
        .saturating_sub(elapsed)
        .max(1);
    intervals
        .checked_sub(1)?
        .checked_mul(bucket.policy.interval_nanos())?
        .checked_add(until_next)
}

fn rejection(
    request: RateLimitRequest<'_>,
    reason: RateLimitRejectReason,
    retry: Option<Duration>,
    remaining: u64,
) -> Result<RateLimitDecision, RateLimitProviderError> {
    RateLimitDecision::rejected(
        request,
        remaining,
        MonotonicTick::from_nanos(0),
        retry,
        reason,
    )
    .map_err(RateLimitProviderError::new)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chigiri_rate::TokenBucketPolicy;

    use super::InMemoryTokenBucket;
    use crate::contention_test;

    #[test]
    fn contended_future_can_be_repeatedly_polled_dropped_and_resumed() {
        let provider = InMemoryTokenBucket::new();
        let guard = provider.buckets.try_lock().unwrap();
        let policy = TokenBucketPolicy::new(1, 1, Duration::from_secs(1)).unwrap();
        contention_test::assert_pending_drop_and_repoll(&provider, policy.into(), guard);
        assert_eq!(provider.entry_count().unwrap(), 1);
    }
}
