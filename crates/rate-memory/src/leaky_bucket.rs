//! Leaky-bucket metering by whole intervals without request queues.

use std::time::Duration;

use chigiri_rate::RateLimitFuture;
use std::sync::TryLockError;

use crate::{AsyncMutex, Clock, InMemoryRateLimitConfig, MonotonicTick, SystemClock};

use super::store::{BoundedStore, LastSeen, observed_now};
use super::{
    BucketLockBusy, BucketLockPoisoned, LeakyBucketPolicy, RateLimitDecision,
    RateLimitProviderError, RateLimitRejectReason, RateLimitRequest, RateLimiter, RatePolicy,
};

#[derive(Clone, Copy)]
struct Meter {
    level: u64,
    last_leak: MonotonicTick,
    last_seen: MonotonicTick,
    policy: LeakyBucketPolicy,
}

impl LastSeen for Meter {
    fn last_seen(&self) -> MonotonicTick {
        self.last_seen
    }
}

/// A bounded meter that rejects requests instead of waiting for capacity.
pub struct InMemoryLeakyBucket<C = crate::SystemClock> {
    clock: C,
    meters: AsyncMutex<BoundedStore<Meter>>,
}

impl<C: Clock> InMemoryLeakyBucket<C> {
    pub fn with_clock(config: InMemoryRateLimitConfig, clock: C) -> Self {
        Self {
            clock,
            meters: AsyncMutex::new(BoundedStore::new(
                config.max_keys(),
                config.idle_ttl_nanos(),
            )),
        }
    }

    async fn decide(
        &self,
        request: RateLimitRequest<'_>,
    ) -> Result<RateLimitDecision, RateLimitProviderError> {
        let RatePolicy::LeakyBucket(policy) = request.policy() else {
            return reject(request, RateLimitRejectReason::PolicyMismatch, None, 0);
        };
        if request.cost().get() > policy.capacity() {
            return reject(
                request,
                RateLimitRejectReason::DynamicCostExceedsLimit,
                None,
                0,
            );
        }
        let now = self.clock.now();
        let mut meters = crate::lock_store(&self.meters).await?;
        meters.maintain_one(now, Meter::last_seen);
        let key = request.key().clone();
        let meter = match meters.get_mut(&key) {
            Some(meter) if meter.policy != policy => {
                return reject(request, RateLimitRejectReason::PolicyMismatch, None, 0);
            }
            Some(meter) => meter,
            None => match meters.insert(
                key,
                Meter {
                    level: 0,
                    last_leak: now,
                    last_seen: now,
                    policy,
                },
            ) {
                Ok(meter) => meter,
                Err(()) => {
                    return reject(
                        request,
                        RateLimitRejectReason::KeyCapacityExhausted,
                        None,
                        0,
                    );
                }
            },
        };
        let effective_now = observed_now(now, meter.last_seen);
        let mut next = *meter;
        drain(&mut next, effective_now);
        next.last_seen = effective_now;
        let Some(candidate) = next.level.checked_add(request.cost().get()) else {
            return reject(
                request,
                RateLimitRejectReason::ArithmeticOverflow,
                None,
                policy.capacity().saturating_sub(next.level),
            );
        };
        if candidate <= policy.capacity() {
            next.level = candidate;
            let decision = RateLimitDecision::allowed(
                request,
                policy.capacity() - candidate,
                effective_now.saturating_add(until_empty(next, effective_now).unwrap_or(u64::MAX)),
            )
            .map_err(RateLimitProviderError::new)?;
            *meter = next;
            Ok(decision)
        } else {
            let needed = candidate - policy.capacity();
            let intervals = needed.div_ceil(policy.units_per_interval());
            let Some(retry) = until_intervals(next, effective_now, intervals) else {
                return reject(
                    request,
                    RateLimitRejectReason::ArithmeticOverflow,
                    None,
                    policy.capacity().saturating_sub(next.level),
                );
            };
            let decision = RateLimitDecision::rejected(
                request,
                policy.capacity().saturating_sub(next.level),
                effective_now.saturating_add(until_empty(next, effective_now).unwrap_or(u64::MAX)),
                Some(Duration::from_nanos(retry)),
                RateLimitRejectReason::QuotaExceeded,
            )
            .map_err(RateLimitProviderError::new)?;
            meter.last_seen = effective_now;
            Ok(decision)
        }
    }

    pub fn entry_count(&self) -> Result<usize, RateLimitProviderError> {
        match self.meters.try_lock() {
            Ok(meters) => Ok(meters.len()),
            Err(TryLockError::Poisoned(_)) => Err(RateLimitProviderError::new(BucketLockPoisoned)),
            Err(TryLockError::WouldBlock) => Err(RateLimitProviderError::new(BucketLockBusy)),
        }
    }
}

impl InMemoryLeakyBucket<SystemClock> {
    pub fn new() -> Self {
        Self::with_config(InMemoryRateLimitConfig::default())
    }

    pub fn with_config(config: InMemoryRateLimitConfig) -> Self {
        Self::with_clock(config, SystemClock::new())
    }
}

impl Default for InMemoryLeakyBucket<SystemClock> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Clock> RateLimiter for InMemoryLeakyBucket<C> {
    fn check_and_consume<'a>(&'a self, request: RateLimitRequest<'a>) -> RateLimitFuture<'a> {
        Box::pin(self.decide(request))
    }
}

fn drain(meter: &mut Meter, now: MonotonicTick) {
    let intervals = now.saturating_sub(meter.last_leak) / meter.policy.interval_nanos();
    if intervals == 0 {
        return;
    }
    let drained = u128::from(intervals) * u128::from(meter.policy.units_per_interval());
    meter.level = u128::from(meter.level).saturating_sub(drained) as u64;
    meter.last_leak = meter
        .last_leak
        .saturating_add(intervals.saturating_mul(meter.policy.interval_nanos()));
}

fn until_empty(meter: Meter, now: MonotonicTick) -> Option<u64> {
    let intervals = meter.level.div_ceil(meter.policy.units_per_interval());
    until_intervals(meter, now, intervals)
}

fn until_intervals(meter: Meter, now: MonotonicTick, intervals: u64) -> Option<u64> {
    if intervals == 0 {
        return Some(0);
    }
    let until_next = meter
        .policy
        .interval_nanos()
        .saturating_sub(now.saturating_sub(meter.last_leak))
        .max(1);
    intervals
        .checked_sub(1)?
        .checked_mul(meter.policy.interval_nanos())?
        .checked_add(until_next)
}

fn reject(
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

    use chigiri_rate::LeakyBucketPolicy;

    use super::InMemoryLeakyBucket;
    use crate::contention_test;

    #[test]
    fn contended_future_can_be_repeatedly_polled_dropped_and_resumed() {
        let provider = InMemoryLeakyBucket::new();
        let guard = provider.meters.try_lock().unwrap();
        let policy = LeakyBucketPolicy::new(1, 1, Duration::from_secs(1)).unwrap();
        contention_test::assert_pending_drop_and_repoll(&provider, policy.into(), guard);
        assert_eq!(provider.entry_count().unwrap(), 1);
    }
}
