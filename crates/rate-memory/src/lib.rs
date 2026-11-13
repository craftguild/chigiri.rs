#![forbid(unsafe_code)]
//! The official in-memory rate limiter does not require async runtime.

mod async_mutex;
mod clock;
mod config;
#[cfg(test)]
mod contention_test;
mod fixed_window;
mod leaky_bucket;
mod sliding_window;
mod store;
mod token_bucket;

use std::error::Error;
use std::fmt;

pub use chigiri_rate::MonotonicTick;
pub use clock::{Clock, ManualClock, SystemClock};
pub use config::{InMemoryRateLimitConfig, InMemoryRateLimitConfigError};
pub use fixed_window::InMemoryFixedWindow;
pub use leaky_bucket::InMemoryLeakyBucket;
pub use sliding_window::InMemorySlidingWindowCounter;
pub use token_bucket::InMemoryTokenBucket;

use chigiri_rate::{
    FixedWindowPolicy, LeakyBucketPolicy, RateLimitDecision, RateLimitProviderError,
    RateLimitRejectReason, RateLimitRequest, RateLimiter, RatePolicy, SlidingWindowPolicy,
    TokenBucketPolicy,
};

use async_mutex::{AsyncMutex, AsyncMutexGuard};

#[derive(Debug)]
struct BucketLockPoisoned;

impl fmt::Display for BucketLockPoisoned {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("rate bucket lock is poisoned")
    }
}

impl Error for BucketLockPoisoned {}

#[derive(Debug)]
struct BucketLockBusy;

impl fmt::Display for BucketLockBusy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("rate bucket lock is busy")
    }
}

impl Error for BucketLockBusy {}

async fn lock_store<T>(
    mutex: &AsyncMutex<T>,
) -> Result<AsyncMutexGuard<'_, T>, RateLimitProviderError> {
    mutex.lock().await
}
