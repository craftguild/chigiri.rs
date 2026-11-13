//! Fixed-window counter that assigns the boundary tick to the new window.

use std::time::Duration;

use chigiri_rate::RateLimitFuture;
use std::sync::TryLockError;

use crate::{AsyncMutex, Clock, InMemoryRateLimitConfig, MonotonicTick, SystemClock};

use super::store::{BoundedStore, LastSeen, observed_now};
use super::{
    BucketLockBusy, BucketLockPoisoned, FixedWindowPolicy, RateLimitDecision,
    RateLimitProviderError, RateLimitRejectReason, RateLimitRequest, RateLimiter, RatePolicy,
};

#[derive(Clone, Copy)]
struct Window {
    index: u64,
    consumed: u64,
    last_seen: MonotonicTick,
    policy: FixedWindowPolicy,
}

impl LastSeen for Window {
    fn last_seen(&self) -> MonotonicTick {
        self.last_seen
    }
}

pub struct InMemoryFixedWindow<C = crate::SystemClock> {
    clock: C,
    windows: AsyncMutex<BoundedStore<Window>>,
}

impl<C: Clock> InMemoryFixedWindow<C> {
    pub fn with_clock(config: InMemoryRateLimitConfig, clock: C) -> Self {
        Self {
            clock,
            windows: AsyncMutex::new(BoundedStore::new(
                config.max_keys(),
                config.idle_ttl_nanos(),
            )),
        }
    }

    async fn decide(
        &self,
        request: RateLimitRequest<'_>,
    ) -> Result<RateLimitDecision, RateLimitProviderError> {
        let RatePolicy::FixedWindow(policy) = request.policy() else {
            return reject(request, RateLimitRejectReason::PolicyMismatch, None, 0);
        };
        let now = self.clock.now();
        if request.cost().get() > policy.limit() {
            return reject(
                request,
                RateLimitRejectReason::DynamicCostExceedsLimit,
                None,
                0,
            );
        }
        let mut windows = crate::lock_store(&self.windows).await?;
        windows.maintain_one(now, Window::last_seen);
        let key = request.key().clone();
        let window = match windows.get_mut(&key) {
            Some(window) if window.policy != policy => {
                return reject(request, RateLimitRejectReason::PolicyMismatch, None, 0);
            }
            Some(window) => window,
            None => {
                let initial = Window {
                    index: now.as_nanos() / policy.window_nanos(),
                    consumed: 0,
                    last_seen: now,
                    policy,
                };
                match windows.insert(key, initial) {
                    Ok(window) => window,
                    Err(()) => {
                        return reject(
                            request,
                            RateLimitRejectReason::KeyCapacityExhausted,
                            None,
                            0,
                        );
                    }
                }
            }
        };
        let effective_now = observed_now(now, window.last_seen);
        let mut next_window = *window;
        let index = effective_now.as_nanos() / policy.window_nanos();
        if index > next_window.index {
            next_window.index = index;
            next_window.consumed = 0;
        }
        next_window.last_seen = effective_now;
        let next = next_window.consumed.checked_add(request.cost().get());
        let boundary = MonotonicTick::from_nanos(
            index
                .saturating_add(1)
                .saturating_mul(policy.window_nanos()),
        );
        if next.is_some_and(|value| value <= policy.limit()) {
            next_window.consumed = next.expect("consumed cost must remain within the limit");
            let decision = RateLimitDecision::allowed(
                request,
                policy.limit() - next_window.consumed,
                boundary,
            )
            .map_err(RateLimitProviderError::new)?;
            *window = next_window;
            Ok(decision)
        } else {
            let retry = boundary
                .as_nanos()
                .saturating_sub(effective_now.as_nanos())
                .max(1);
            let decision = RateLimitDecision::rejected(
                request,
                policy.limit().saturating_sub(next_window.consumed),
                boundary,
                Some(Duration::from_nanos(retry)),
                RateLimitRejectReason::QuotaExceeded,
            )
            .map_err(RateLimitProviderError::new)?;
            window.last_seen = effective_now;
            Ok(decision)
        }
    }

    pub fn entry_count(&self) -> Result<usize, RateLimitProviderError> {
        match self.windows.try_lock() {
            Ok(windows) => Ok(windows.len()),
            Err(TryLockError::Poisoned(_)) => Err(RateLimitProviderError::new(BucketLockPoisoned)),
            Err(TryLockError::WouldBlock) => Err(RateLimitProviderError::new(BucketLockBusy)),
        }
    }
}

impl InMemoryFixedWindow<SystemClock> {
    pub fn new() -> Self {
        Self::with_config(InMemoryRateLimitConfig::default())
    }

    pub fn with_config(config: InMemoryRateLimitConfig) -> Self {
        Self::with_clock(config, SystemClock::new())
    }
}

impl Default for InMemoryFixedWindow<SystemClock> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Clock> RateLimiter for InMemoryFixedWindow<C> {
    fn check_and_consume<'a>(&'a self, request: RateLimitRequest<'a>) -> RateLimitFuture<'a> {
        Box::pin(self.decide(request))
    }
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

    use chigiri_rate::FixedWindowPolicy;

    use super::InMemoryFixedWindow;
    use crate::contention_test;

    #[test]
    fn contended_future_can_be_repeatedly_polled_dropped_and_resumed() {
        let provider = InMemoryFixedWindow::new();
        let guard = provider.windows.try_lock().unwrap();
        let policy = FixedWindowPolicy::new(1, Duration::from_secs(1)).unwrap();
        contention_test::assert_pending_drop_and_repoll(&provider, policy.into(), guard);
        assert_eq!(provider.entry_count().unwrap(), 1);
    }
}
