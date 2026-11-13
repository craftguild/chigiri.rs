//! Integer-weighted counter spanning the previous and current windows.

use std::time::Duration;

use chigiri_rate::RateLimitFuture;
use std::sync::TryLockError;

use crate::{AsyncMutex, Clock, InMemoryRateLimitConfig, MonotonicTick, SystemClock};

use super::store::{BoundedStore, LastSeen, observed_now};
use super::{
    BucketLockBusy, BucketLockPoisoned, RateLimitDecision, RateLimitProviderError,
    RateLimitRejectReason, RateLimitRequest, RateLimiter, RatePolicy, SlidingWindowPolicy,
};

#[derive(Clone, Copy)]
struct Counter {
    index: u64,
    previous: u64,
    current: u64,
    last_seen: MonotonicTick,
    policy: SlidingWindowPolicy,
}

impl LastSeen for Counter {
    fn last_seen(&self) -> MonotonicTick {
        self.last_seen
    }
}

/// A sliding-window provider that uses integer arithmetic only.
pub struct InMemorySlidingWindowCounter<C = crate::SystemClock> {
    clock: C,
    counters: AsyncMutex<BoundedStore<Counter>>,
}

impl<C: Clock> InMemorySlidingWindowCounter<C> {
    pub fn with_clock(config: InMemoryRateLimitConfig, clock: C) -> Self {
        Self {
            clock,
            counters: AsyncMutex::new(BoundedStore::new(
                config.max_keys(),
                config.idle_ttl_nanos(),
            )),
        }
    }

    async fn decide(
        &self,
        request: RateLimitRequest<'_>,
    ) -> Result<RateLimitDecision, RateLimitProviderError> {
        let RatePolicy::SlidingWindowCounter(policy) = request.policy() else {
            return reject(request, RateLimitRejectReason::PolicyMismatch, None, 0);
        };
        if request.cost().get() > policy.limit() {
            return reject(
                request,
                RateLimitRejectReason::DynamicCostExceedsLimit,
                None,
                0,
            );
        }
        let now = self.clock.now();
        let mut counters = crate::lock_store(&self.counters).await?;
        counters.maintain_one(now, Counter::last_seen);
        let key = request.key().clone();
        let counter = match counters.get_mut(&key) {
            Some(counter) if counter.policy != policy => {
                return reject(request, RateLimitRejectReason::PolicyMismatch, None, 0);
            }
            Some(counter) => counter,
            None => match counters.insert(
                key,
                Counter {
                    index: now.as_nanos() / policy.window_nanos(),
                    previous: 0,
                    current: 0,
                    last_seen: now,
                    policy,
                },
            ) {
                Ok(counter) => counter,
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
        let effective_now = observed_now(now, counter.last_seen);
        let mut next = *counter;
        rotate(&mut next, effective_now);
        next.last_seen = effective_now;
        let scaled = match scaled_values(next, effective_now, request.cost().get()) {
            Some(values) => values,
            None => {
                return reject(request, RateLimitRejectReason::ArithmeticOverflow, None, 0);
            }
        };
        let remaining = scaled
            .limit
            .saturating_sub(scaled.estimated)
            .checked_div(u128::from(policy.window_nanos()))
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or(0);
        if scaled.candidate <= scaled.limit {
            let Some(current) = next.current.checked_add(request.cost().get()) else {
                return reject(
                    request,
                    RateLimitRejectReason::ArithmeticOverflow,
                    None,
                    remaining,
                );
            };
            next.current = current;
            let decision = RateLimitDecision::allowed(
                request,
                remaining.saturating_sub(request.cost().get()),
                next_boundary(next),
            )
            .map_err(RateLimitProviderError::new)?;
            *counter = next;
            Ok(decision)
        } else {
            let Some(retry) = retry_after(next, effective_now, request.cost().get()) else {
                return reject(
                    request,
                    RateLimitRejectReason::ArithmeticOverflow,
                    None,
                    remaining,
                );
            };
            let decision = RateLimitDecision::rejected(
                request,
                remaining,
                effective_now.saturating_add(retry),
                Some(Duration::from_nanos(retry)),
                RateLimitRejectReason::QuotaExceeded,
            )
            .map_err(RateLimitProviderError::new)?;
            counter.last_seen = effective_now;
            Ok(decision)
        }
    }

    pub fn entry_count(&self) -> Result<usize, RateLimitProviderError> {
        match self.counters.try_lock() {
            Ok(counters) => Ok(counters.len()),
            Err(TryLockError::Poisoned(_)) => Err(RateLimitProviderError::new(BucketLockPoisoned)),
            Err(TryLockError::WouldBlock) => Err(RateLimitProviderError::new(BucketLockBusy)),
        }
    }
}

impl InMemorySlidingWindowCounter<SystemClock> {
    pub fn new() -> Self {
        Self::with_config(InMemoryRateLimitConfig::default())
    }

    pub fn with_config(config: InMemoryRateLimitConfig) -> Self {
        Self::with_clock(config, SystemClock::new())
    }
}

impl Default for InMemorySlidingWindowCounter<SystemClock> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Clock> RateLimiter for InMemorySlidingWindowCounter<C> {
    fn check_and_consume<'a>(&'a self, request: RateLimitRequest<'a>) -> RateLimitFuture<'a> {
        Box::pin(self.decide(request))
    }
}

struct Scaled {
    estimated: u128,
    candidate: u128,
    limit: u128,
}

fn rotate(counter: &mut Counter, now: MonotonicTick) {
    let index = now.as_nanos() / counter.policy.window_nanos();
    match index.saturating_sub(counter.index) {
        0 => {}
        1 => {
            counter.previous = counter.current;
            counter.current = 0;
            counter.index = index;
        }
        _ => {
            counter.previous = 0;
            counter.current = 0;
            counter.index = index;
        }
    }
}

fn scaled_values(counter: Counter, now: MonotonicTick, cost: u64) -> Option<Scaled> {
    let width = u128::from(counter.policy.window_nanos());
    let elapsed = u128::from(now.as_nanos() % counter.policy.window_nanos());
    let current = u128::from(counter.current).checked_mul(width)?;
    let previous = u128::from(counter.previous).checked_mul(width.checked_sub(elapsed)?)?;
    let estimated = current.checked_add(previous)?;
    let candidate = estimated.checked_add(u128::from(cost).checked_mul(width)?)?;
    let limit = u128::from(counter.policy.limit()).checked_mul(width)?;
    Some(Scaled {
        estimated,
        candidate,
        limit,
    })
}

fn retry_after(counter: Counter, now: MonotonicTick, cost: u64) -> Option<u64> {
    let width = u128::from(counter.policy.window_nanos());
    let elapsed = u128::from(now.as_nanos() % counter.policy.window_nanos());
    let scaled = scaled_values(counter, now, cost)?;
    let excess = scaled.candidate.checked_sub(scaled.limit)?;
    if counter.previous > 0 {
        let delta = div_ceil(excess, u128::from(counter.previous))?;
        if elapsed.checked_add(delta)? <= width {
            return u64::try_from(delta.max(1)).ok();
        }
    }
    let to_boundary = width.checked_sub(elapsed)?;
    let at_boundary = u128::from(counter.current).checked_add(u128::from(cost))?;
    if at_boundary <= u128::from(counter.policy.limit()) {
        return u64::try_from(to_boundary.max(1)).ok();
    }
    let decay = at_boundary.checked_sub(u128::from(counter.policy.limit()))?;
    let after_boundary = div_ceil(decay.checked_mul(width)?, u128::from(counter.current))?;
    u64::try_from(to_boundary.checked_add(after_boundary)?.max(1)).ok()
}

fn div_ceil(numerator: u128, denominator: u128) -> Option<u128> {
    let quotient = numerator.checked_div(denominator)?;
    let remainder = numerator.checked_rem(denominator)?;
    quotient.checked_add(u128::from(remainder > 0))
}

fn next_boundary(counter: Counter) -> MonotonicTick {
    MonotonicTick::from_nanos(
        counter
            .index
            .saturating_add(1)
            .saturating_mul(counter.policy.window_nanos()),
    )
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

    use chigiri_rate::SlidingWindowPolicy;

    use super::InMemorySlidingWindowCounter;
    use crate::contention_test;

    #[test]
    fn contended_future_can_be_repeatedly_polled_dropped_and_resumed() {
        let provider = InMemorySlidingWindowCounter::new();
        let guard = provider.counters.try_lock().unwrap();
        let policy = SlidingWindowPolicy::new(1, Duration::from_secs(1)).unwrap();
        contention_test::assert_pending_drop_and_repoll(&provider, policy.into(), guard);
        assert_eq!(provider.entry_count().unwrap(), 1);
    }
}
