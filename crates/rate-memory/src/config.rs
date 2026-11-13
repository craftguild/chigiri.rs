//! Validated capacity/expiry settings shared by all four providers.

use std::fmt;
use std::time::Duration;

const DEFAULT_MAX_KEYS: usize = 10_000;
const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(300);

/// Provider configuration for a bounded store, immutable after construction.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct InMemoryRateLimitConfig {
    max_keys: usize,
    idle_ttl_nanos: u64,
}

impl InMemoryRateLimitConfig {
    pub fn new(max_keys: usize, idle_ttl: Duration) -> Result<Self, InMemoryRateLimitConfigError> {
        let nanos = idle_ttl.as_nanos();
        if max_keys == 0 || nanos == 0 || nanos > u128::from(u64::MAX) {
            return Err(InMemoryRateLimitConfigError);
        }
        Ok(Self {
            max_keys,
            idle_ttl_nanos: nanos as u64,
        })
    }

    pub const fn max_keys(self) -> usize {
        self.max_keys
    }

    pub const fn idle_ttl(self) -> Duration {
        Duration::from_nanos(self.idle_ttl_nanos)
    }

    pub(crate) const fn idle_ttl_nanos(self) -> u64 {
        self.idle_ttl_nanos
    }
}

impl Default for InMemoryRateLimitConfig {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_KEYS, DEFAULT_IDLE_TTL)
            .expect("fixed in-memory defaults must always be valid")
    }
}

impl fmt::Debug for InMemoryRateLimitConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InMemoryRateLimitConfig")
            .finish_non_exhaustive()
    }
}

/// Error for a zero configuration or one that cannot be represented by
/// internal ticks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InMemoryRateLimitConfigError;

impl fmt::Display for InMemoryRateLimitConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("in-memory rate limit config is invalid")
    }
}

impl std::error::Error for InMemoryRateLimitConfigError {}
