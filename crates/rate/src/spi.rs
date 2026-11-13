//! Provider SPI for borrowed requests and atomic decisions.

use std::error::Error;
use std::fmt;

use crate::{OperationCost, QuotaKey, RateLimitDecision, RatePolicy};

/// A borrowed, `Send` future returned by a rate-limit provider.
pub type RateLimitFuture<'a> = std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<RateLimitDecision, RateLimitProviderError>>
            + Send
            + 'a,
    >,
>;

/// Borrowed request for an atomic decision.
#[derive(Clone, Copy, Debug)]
pub struct RateLimitRequest<'a> {
    key: &'a QuotaKey,
    cost: OperationCost,
    policy: RatePolicy,
}

impl<'a> RateLimitRequest<'a> {
    pub const fn new(key: &'a QuotaKey, cost: OperationCost, policy: RatePolicy) -> Self {
        Self { key, cost, policy }
    }
    pub const fn key(&self) -> &'a QuotaKey {
        self.key
    }
    pub const fn cost(&self) -> OperationCost {
        self.cost
    }
    pub const fn policy(&self) -> RatePolicy {
        self.policy
    }
}

/// Error that does not expose a provider-specific source to clients or through
/// `Debug` or `Display`.
pub struct RateLimitProviderError(Box<dyn Error + Send + Sync>);

impl RateLimitProviderError {
    pub fn new(source: impl Error + Send + Sync + 'static) -> Self {
        Self(Box::new(source))
    }
}

impl fmt::Debug for RateLimitProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RateLimitProviderError")
            .finish_non_exhaustive()
    }
}

impl fmt::Display for RateLimitProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("rate limit provider failed")
    }
}

impl Error for RateLimitProviderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// Object-safe provider SPI that evaluates quota and consumption atomically.
pub trait RateLimiter: Send + Sync + 'static {
    fn check_and_consume<'a>(&'a self, request: RateLimitRequest<'a>) -> RateLimitFuture<'a>;
}
