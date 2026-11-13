#![forbid(unsafe_code)]
//! Atomic rate-limit provider SPI independent of runtime, HTTP, and storage implementations.

mod cost;
mod key;
mod policy;
mod snapshot;
mod spi;
mod tick;

pub use cost::{OperationCost, OperationCostError};
pub use key::{QuotaKey, QuotaKeyError, QuotaSubject};
pub use policy::{
    FixedWindowPolicy, LeakyBucketPolicy, PolicyRevision, RateLimitPolicyError, RatePolicy,
    SlidingWindowPolicy, TokenBucketPolicy,
};
pub use snapshot::{
    RateLimitDecision, RateLimitDecisionError, RateLimitOutcome, RateLimitRejectReason,
    RateLimitSnapshot,
};
pub use spi::{RateLimitFuture, RateLimitProviderError, RateLimitRequest, RateLimiter};
pub use tick::MonotonicTick;
