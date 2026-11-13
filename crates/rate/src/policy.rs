//! Sealed policy types that make invalid algorithm combinations unrepresentable.

use std::fmt;
use std::time::Duration;

/// Opaque policy revision connecting readiness and decisions.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PolicyRevision(u64);

impl fmt::Debug for PolicyRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PolicyRevision(..)")
    }
}

impl PolicyRevision {
    pub const fn new(value: u64) -> Result<Self, RateLimitPolicyError> {
        if value == 0 {
            Err(RateLimitPolicyError)
        } else {
            Ok(Self(value))
        }
    }
    pub const fn initial() -> Self {
        Self(1)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}

macro_rules! interval_policy {
    ($name:ident, $first:ident, $second:ident, $second_getter:ident) => {
        #[derive(Clone, Copy, Eq, PartialEq)]
        pub struct $name {
            pub(crate) $first: u64,
            pub(crate) $second: u64,
            pub(crate) interval_nanos: u64,
            pub(crate) revision: PolicyRevision,
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($name))
                    .finish_non_exhaustive()
            }
        }

        impl $name {
            pub fn new(
                $first: u64,
                $second: u64,
                interval: Duration,
            ) -> Result<Self, RateLimitPolicyError> {
                let interval_nanos = checked_duration(interval)?;
                if $first == 0 || $second == 0 {
                    return Err(RateLimitPolicyError);
                }
                Ok(Self {
                    $first,
                    $second,
                    interval_nanos,
                    revision: PolicyRevision::initial(),
                })
            }

            pub const fn with_revision(mut self, revision: PolicyRevision) -> Self {
                self.revision = revision;
                self
            }

            pub const fn capacity(self) -> u64 {
                self.$first
            }

            pub const fn $second_getter(self) -> u64 {
                self.$second
            }

            pub const fn interval(self) -> Duration {
                Duration::from_nanos(self.interval_nanos)
            }

            #[doc(hidden)]
            pub const fn interval_nanos(self) -> u64 {
                self.interval_nanos
            }

            pub const fn revision(self) -> PolicyRevision {
                self.revision
            }
        }
    };
}

interval_policy!(
    TokenBucketPolicy,
    capacity,
    tokens_per_interval,
    tokens_per_interval
);
interval_policy!(
    LeakyBucketPolicy,
    capacity,
    units_per_interval,
    units_per_interval
);

/// The limit and width of the fixed window.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct FixedWindowPolicy {
    pub(crate) limit: u64,
    pub(crate) window_nanos: u64,
    pub(crate) revision: PolicyRevision,
}

impl fmt::Debug for FixedWindowPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FixedWindowPolicy")
            .finish_non_exhaustive()
    }
}

impl FixedWindowPolicy {
    pub fn new(limit: u64, window: Duration) -> Result<Self, RateLimitPolicyError> {
        if limit == 0 {
            return Err(RateLimitPolicyError);
        }
        Ok(Self {
            limit,
            window_nanos: checked_duration(window)?,
            revision: PolicyRevision::initial(),
        })
    }
    pub const fn with_revision(mut self, revision: PolicyRevision) -> Self {
        self.revision = revision;
        self
    }
    pub const fn limit(self) -> u64 {
        self.limit
    }
    pub const fn window(self) -> Duration {
        Duration::from_nanos(self.window_nanos)
    }
    #[doc(hidden)]
    pub const fn window_nanos(self) -> u64 {
        self.window_nanos
    }
    pub const fn revision(self) -> PolicyRevision {
        self.revision
    }
}

/// The limit and width of the sliding-window counter.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SlidingWindowPolicy {
    pub(crate) limit: u64,
    pub(crate) window_nanos: u64,
    pub(crate) revision: PolicyRevision,
}

impl fmt::Debug for SlidingWindowPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SlidingWindowPolicy")
            .finish_non_exhaustive()
    }
}

impl SlidingWindowPolicy {
    pub fn new(limit: u64, window: Duration) -> Result<Self, RateLimitPolicyError> {
        if limit == 0 {
            return Err(RateLimitPolicyError);
        }
        Ok(Self {
            limit,
            window_nanos: checked_duration(window)?,
            revision: PolicyRevision::initial(),
        })
    }
    pub const fn with_revision(mut self, revision: PolicyRevision) -> Self {
        self.revision = revision;
        self
    }
    pub const fn limit(self) -> u64 {
        self.limit
    }
    pub const fn window(self) -> Duration {
        Duration::from_nanos(self.window_nanos)
    }
    #[doc(hidden)]
    pub const fn window_nanos(self) -> u64 {
        self.window_nanos
    }
    pub const fn revision(self) -> PolicyRevision {
        self.revision
    }
}

/// The concrete algorithm policy selected for a request.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum RatePolicy {
    TokenBucket(TokenBucketPolicy),
    FixedWindow(FixedWindowPolicy),
    SlidingWindowCounter(SlidingWindowPolicy),
    LeakyBucket(LeakyBucketPolicy),
}

impl fmt::Debug for RatePolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TokenBucket(_) => "TokenBucket(..)",
            Self::FixedWindow(_) => "FixedWindow(..)",
            Self::SlidingWindowCounter(_) => "SlidingWindowCounter(..)",
            Self::LeakyBucket(_) => "LeakyBucket(..)",
        })
    }
}

impl RatePolicy {
    pub const fn limit(self) -> u64 {
        match self {
            Self::TokenBucket(policy) => policy.capacity,
            Self::FixedWindow(policy) => policy.limit,
            Self::SlidingWindowCounter(policy) => policy.limit,
            Self::LeakyBucket(policy) => policy.capacity,
        }
    }
    pub const fn revision(self) -> PolicyRevision {
        match self {
            Self::TokenBucket(policy) => policy.revision,
            Self::FixedWindow(policy) => policy.revision,
            Self::SlidingWindowCounter(policy) => policy.revision,
            Self::LeakyBucket(policy) => policy.revision,
        }
    }
}

impl From<TokenBucketPolicy> for RatePolicy {
    fn from(value: TokenBucketPolicy) -> Self {
        Self::TokenBucket(value)
    }
}
impl From<FixedWindowPolicy> for RatePolicy {
    fn from(value: FixedWindowPolicy) -> Self {
        Self::FixedWindow(value)
    }
}
impl From<SlidingWindowPolicy> for RatePolicy {
    fn from(value: SlidingWindowPolicy) -> Self {
        Self::SlidingWindowCounter(value)
    }
}
impl From<LeakyBucketPolicy> for RatePolicy {
    fn from(value: LeakyBucketPolicy) -> Self {
        Self::LeakyBucket(value)
    }
}

/// Error for an invalid, zero, or overflowing rate policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateLimitPolicyError;

impl fmt::Display for RateLimitPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("rate limit policy values must be finite and greater than zero")
    }
}

impl std::error::Error for RateLimitPolicyError {}

fn checked_duration(duration: Duration) -> Result<u64, RateLimitPolicyError> {
    let nanos = duration.as_nanos();
    if nanos == 0 || nanos > u64::MAX.into() {
        Err(RateLimitPolicyError)
    } else {
        Ok(nanos as u64)
    }
}
