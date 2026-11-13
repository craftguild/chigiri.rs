#![forbid(unsafe_code)]
//! Minimal third-party provider that depends only on `chigiri_rate`.

use std::sync::atomic::{AtomicU64, Ordering};

use chigiri_rate::{
    MonotonicTick, RateLimitDecision, RateLimitFuture, RateLimitProviderError,
    RateLimitRejectReason, RateLimitRequest, RateLimiter,
};

/// One policy is the smallest provider to handle one atomic counter.
pub struct AtomicCounterLimiter {
    consumed: AtomicU64,
}

impl AtomicCounterLimiter {
    pub const fn new() -> Self {
        Self {
            consumed: AtomicU64::new(0),
        }
    }
}

impl Default for AtomicCounterLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter for AtomicCounterLimiter {
    fn check_and_consume<'a>(&'a self, request: RateLimitRequest<'a>) -> RateLimitFuture<'a> {
        Box::pin(async move {
            let limit = request.policy().limit();
            let cost = request.cost().get();
            let consumed =
                self.consumed
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |previous| {
                        previous
                            .checked_add(cost)
                            .filter(|candidate| *candidate <= limit)
                    });
            match consumed {
                Ok(previous) => allowed(request, limit - previous - cost),
                Err(previous) => rejected(request, limit.saturating_sub(previous)),
            }
        })
    }
}

fn allowed(
    request: RateLimitRequest<'_>,
    remaining: u64,
) -> Result<RateLimitDecision, RateLimitProviderError> {
    RateLimitDecision::allowed(
        request,
        remaining,
        MonotonicTick::from_nanos(1),
    )
    .map_err(RateLimitProviderError::new)
}

fn rejected(
    request: RateLimitRequest<'_>,
    remaining: u64,
) -> Result<RateLimitDecision, RateLimitProviderError> {
    RateLimitDecision::rejected(
        request,
        remaining,
        MonotonicTick::from_nanos(1),
        Some(std::time::Duration::from_nanos(1)),
        RateLimitRejectReason::QuotaExceeded,
    )
    .map_err(RateLimitProviderError::new)
}
