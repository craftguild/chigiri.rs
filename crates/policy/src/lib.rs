#![forbid(unsafe_code)]
//! Separates operation-cost and quota providers from runtime and HTTP layers.

mod cost;
mod failure_policy;
mod observer_binding;
mod provider_timeout;
mod stage_a;
mod stage_b;

pub use cost::{
    CostPolicyConfigurationError, OperationCostContext, OperationCostPolicy, StandardOperationCost,
    UnitFieldCost,
};
pub use failure_policy::ProviderFailurePolicy;
pub use observer_binding::RateLimitObserverBinding;
pub use provider_timeout::{ProviderTimeoutConfigurationError, TimedRateLimiter};
pub use stage_a::{StageARateLimit, TransportQuotaExtractor};
pub use stage_b::{OperationQuotaExtractor, OperationRateLimit};

use chigiri_rate::{
    OperationCost, QuotaKey, RateLimitDecision, RateLimitPolicyError, RateLimitProviderError,
    RateLimitRequest, RateLimitSnapshot, RateLimiter, RatePolicy,
};
