//! Bounded decision snapshots that contain no secret metadata.

use std::fmt;
use std::time::Duration;

use crate::{MonotonicTick, PolicyRevision, RateLimitRequest};

/// The reason the provider explicitly refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateLimitRejectReason {
    QuotaExceeded,
    DynamicCostExceedsLimit,
    KeyCapacityExhausted,
    PolicyMismatch,
    ArithmeticOverflow,
}

/// Metadata inconsistency detected in decision-making or provider boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateLimitDecisionError;

impl fmt::Display for RateLimitDecisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("rate limit decision metadata is inconsistent")
    }
}

impl std::error::Error for RateLimitDecisionError {}

/// Bounded public metadata shared by allow and reject decisions.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct RateLimitSnapshot {
    limit: u64,
    remaining: u64,
    reset_at: MonotonicTick,
    retry_after: Option<Duration>,
    revision: PolicyRevision,
    reject_reason: Option<RateLimitRejectReason>,
}

impl fmt::Debug for RateLimitSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RateLimitSnapshot")
            .field("reject_reason", &self.reject_reason)
            .field("has_retry_after", &self.retry_after.is_some())
            .finish_non_exhaustive()
    }
}

impl RateLimitSnapshot {
    fn new(
        request: RateLimitRequest<'_>,
        remaining: u64,
        reset_at: MonotonicTick,
        retry_after: Option<Duration>,
        reject_reason: Option<RateLimitRejectReason>,
    ) -> Result<Self, RateLimitDecisionError> {
        let limit = request.policy().limit();
        if remaining > limit
            || retry_after
                .is_some_and(|value| value.is_zero() || value.as_nanos() > u128::from(u64::MAX))
            || (reject_reason.is_none() && retry_after.is_some())
            || (reject_reason == Some(RateLimitRejectReason::QuotaExceeded)
                && retry_after.is_none())
        {
            return Err(RateLimitDecisionError);
        }
        Ok(Self {
            limit,
            remaining,
            reset_at,
            retry_after,
            revision: request.policy().revision(),
            reject_reason,
        })
    }

    pub const fn limit(self) -> u64 {
        self.limit
    }
    pub const fn remaining(self) -> u64 {
        self.remaining
    }
    pub const fn reset_at(self) -> MonotonicTick {
        self.reset_at
    }
    pub const fn retry_after(self) -> Option<Duration> {
        self.retry_after
    }
    pub const fn revision(self) -> PolicyRevision {
        self.revision
    }
    pub const fn reject_reason(self) -> Option<RateLimitRejectReason> {
        self.reject_reason
    }
}

/// The closed outcome of the decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateLimitOutcome {
    Allowed,
    Rejected,
}

/// An atomic provider decision that keeps its outcome and snapshot consistent.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct RateLimitDecision {
    outcome: RateLimitOutcome,
    snapshot: RateLimitSnapshot,
}

impl RateLimitDecision {
    pub fn allowed(
        request: RateLimitRequest<'_>,
        remaining: u64,
        reset_at: MonotonicTick,
    ) -> Result<Self, RateLimitDecisionError> {
        if request.cost().get() > request.policy().limit() {
            return Err(RateLimitDecisionError);
        }
        Ok(Self {
            outcome: RateLimitOutcome::Allowed,
            snapshot: RateLimitSnapshot::new(request, remaining, reset_at, None, None)?,
        })
    }

    pub fn rejected(
        request: RateLimitRequest<'_>,
        remaining: u64,
        reset_at: MonotonicTick,
        retry_after: Option<Duration>,
        reason: RateLimitRejectReason,
    ) -> Result<Self, RateLimitDecisionError> {
        Ok(Self {
            outcome: RateLimitOutcome::Rejected,
            snapshot: RateLimitSnapshot::new(
                request,
                remaining,
                reset_at,
                retry_after,
                Some(reason),
            )?,
        })
    }

    /// Revalidates provider output against the policy and request metadata.
    pub fn validate_for(
        self,
        request: RateLimitRequest<'_>,
    ) -> Result<Self, RateLimitDecisionError> {
        let crossed = match self.outcome {
            RateLimitOutcome::Allowed => {
                self.snapshot.reject_reason.is_some() || self.snapshot.retry_after.is_some()
            }
            RateLimitOutcome::Rejected => {
                self.snapshot.reject_reason.is_none()
                    || (self.snapshot.reject_reason == Some(RateLimitRejectReason::QuotaExceeded)
                        && self.snapshot.retry_after.is_none())
            }
        };
        if crossed
            || self.snapshot.limit != request.policy().limit()
            || self.snapshot.revision != request.policy().revision()
            || (self.outcome == RateLimitOutcome::Allowed
                && request.cost().get() > request.policy().limit())
        {
            Err(RateLimitDecisionError)
        } else {
            Ok(self)
        }
    }

    pub const fn outcome(self) -> RateLimitOutcome {
        self.outcome
    }

    pub const fn is_allowed(self) -> bool {
        matches!(self.outcome, RateLimitOutcome::Allowed)
    }

    pub const fn snapshot(self) -> RateLimitSnapshot {
        self.snapshot
    }
}

impl fmt::Debug for RateLimitDecision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.outcome {
            RateLimitOutcome::Allowed => formatter
                .debug_tuple("Allowed")
                .field(&self.snapshot)
                .finish(),
            RateLimitOutcome::Rejected => formatter
                .debug_tuple("Rejected")
                .field(&self.snapshot)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OperationCost, QuotaKey, QuotaSubject, RatePolicy, TokenBucketPolicy};

    #[test]
    fn rejects_decisions_with_inconsistent_outcomes_or_requests() {
        let key = QuotaKey::new(b"test".to_vec(), QuotaSubject::Missing, None).unwrap();
        let request = RateLimitRequest::new(&key, OperationCost::one(), policy(10));
        let rejected = RateLimitSnapshot::new(
            request,
            0,
            MonotonicTick::from_nanos(1),
            None,
            Some(RateLimitRejectReason::PolicyMismatch),
        )
        .unwrap();
        let crossed = RateLimitDecision {
            outcome: RateLimitOutcome::Allowed,
            snapshot: rejected,
        };
        assert_eq!(crossed.validate_for(request), Err(RateLimitDecisionError));

        let other_request = RateLimitRequest::new(&key, OperationCost::one(), policy(11));
        let decision =
            RateLimitDecision::allowed(other_request, 10, MonotonicTick::from_nanos(1)).unwrap();
        assert_eq!(decision.validate_for(request), Err(RateLimitDecisionError));
    }

    fn policy(capacity: u64) -> RatePolicy {
        TokenBucketPolicy::new(capacity, 1, Duration::from_secs(1))
            .unwrap()
            .into()
    }
}
