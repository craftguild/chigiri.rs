//! Verify that closed rejection cases do not mutate existing accounting.

use chigiri_rate::{RateLimitDecision, RateLimitRejectReason, RateLimitRequest, RateLimiter};

use super::{ExpectedRateDecision, validate_decision};
use crate::provider::ProviderConformanceError;

/// Fixtures that apply a closed rejection after a prior allowed consume.
#[derive(Clone, Copy, Debug)]
pub struct RateProviderStateCase<'a> {
    pub initial: RateLimitRequest<'a>,
    pub rejected: RateLimitRequest<'a>,
    pub follow_up: RateLimitRequest<'a>,
    pub reason: RateLimitRejectReason,
}

/// Verifies that capacity, policy-mismatch, and dynamic over-cost rejections do
/// not change accounting.
pub async fn assert_rate_provider_state_conformance(
    provider: &dyn RateLimiter,
    case: RateProviderStateCase<'_>,
) -> Result<(), ProviderConformanceError> {
    let initial = provider
        .check_and_consume(case.initial)
        .await
        .map_err(|_| ProviderConformanceError::new("initial rate provider call failed"))?;
    validate_decision(initial, case.initial, Some(ExpectedRateDecision::Allowed))?;
    let initial_remaining = snapshot(initial).remaining();

    let rejected = provider
        .check_and_consume(case.rejected)
        .await
        .map_err(|_| ProviderConformanceError::new("rejected rate provider call failed"))?;
    validate_decision(
        rejected,
        case.rejected,
        Some(ExpectedRateDecision::Rejected),
    )?;
    if snapshot(rejected).reject_reason() != Some(case.reason) {
        return Err(ProviderConformanceError::new(
            "rate provider rejection reason is inconsistent",
        ));
    }

    let follow_up = provider
        .check_and_consume(case.follow_up)
        .await
        .map_err(|_| ProviderConformanceError::new("follow-up rate provider call failed"))?;
    validate_decision(
        follow_up,
        case.follow_up,
        Some(ExpectedRateDecision::Allowed),
    )?;
    let expected = initial_remaining
        .checked_sub(case.follow_up.cost().get())
        .ok_or_else(|| ProviderConformanceError::new("state fixture cost is inconsistent"))?;
    if snapshot(follow_up).remaining() != expected {
        return Err(ProviderConformanceError::new(
            "rejected rate call changed provider accounting",
        ));
    }
    Ok(())
}

fn snapshot(decision: RateLimitDecision) -> chigiri_rate::RateLimitSnapshot {
    decision.snapshot()
}
