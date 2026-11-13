//! Connects official algorithms and Stage A/B to the shared component graph.

mod algorithm;
mod stage;

pub use algorithm::{
    AlgorithmBuilder, FixedWindow, LeakyBucket, Operation, OperationBuilder, SlidingWindow,
    TokenBucket, Transport, TransportBuilder, operation, transport,
};
pub use chigiri_policy::{
    OperationCostContext, OperationCostPolicy, OperationQuotaExtractor, OperationRateLimit,
    ProviderFailurePolicy, RateLimitObserverBinding, StageARateLimit, StandardOperationCost,
    TimedRateLimiter, TransportQuotaExtractor, UnitFieldCost,
};
pub use chigiri_rate::{
    FixedWindowPolicy, LeakyBucketPolicy, MonotonicTick, OperationCost, OperationCostError,
    PolicyRevision, QuotaKey, QuotaKeyError, QuotaSubject, RateLimitDecision, RateLimitFuture,
    RateLimitPolicyError, RateLimitProviderError, RateLimitRejectReason, RateLimitRequest,
    RateLimitSnapshot, RateLimiter, RatePolicy, SlidingWindowPolicy, TokenBucketPolicy,
};
pub use stage::{
    OperationRateComponent, OperationRateStage, RateStage, TransportRateComponent,
    TransportRateStage,
};

/// The official process-local provider.
pub mod memory {
    pub use chigiri_rate_memory::{
        Clock, InMemoryFixedWindow, InMemoryLeakyBucket, InMemoryRateLimitConfig,
        InMemoryRateLimitConfigError, InMemorySlidingWindowCounter, InMemoryTokenBucket,
        ManualClock, MonotonicTick, SystemClock,
    };
}

use std::fmt;

/// A construction error indicating a lack of essential values or a bounded policy violation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateStageBuildError {
    /// The selected algorithm is missing a required setting.
    MissingSetting(&'static str),
    /// The value of the quota policy is zero or out of range.
    InvalidPolicy(RateLimitPolicyError),
    /// The provider deadline is zero or outside the representable range.
    InvalidProviderTimeout(chigiri_policy::ProviderTimeoutConfigurationError),
}

impl fmt::Display for RateStageBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSetting(name) => write!(formatter, "rate setting `{name}` is required"),
            Self::InvalidPolicy(error) => error.fmt(formatter),
            Self::InvalidProviderTimeout(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RateStageBuildError {}

impl From<RateLimitPolicyError> for RateStageBuildError {
    fn from(value: RateLimitPolicyError) -> Self {
        Self::InvalidPolicy(value)
    }
}

impl From<chigiri_policy::ProviderTimeoutConfigurationError> for RateStageBuildError {
    fn from(value: chigiri_policy::ProviderTimeoutConfigurationError) -> Self {
        Self::InvalidProviderTimeout(value)
    }
}

pub(crate) use stage::apply_stage;
