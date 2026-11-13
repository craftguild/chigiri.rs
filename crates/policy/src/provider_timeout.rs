//! Runtime-neutral deadlines for rate-provider futures.

use std::future::poll_fn;
use std::task::Poll;
use std::time::Duration;

use chigiri_http::BoxFuture;
use chigiri_runtime::core::DeadlineFactory;

use crate::{RateLimitDecision, RateLimitProviderError, RateLimitRequest, RateLimiter};

/// A provider timeout was zero or outside the supported tick range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderTimeoutConfigurationError;

impl std::fmt::Display for ProviderTimeoutConfigurationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("rate provider timeout must be bounded and greater than zero")
    }
}

impl std::error::Error for ProviderTimeoutConfigurationError {}

/// Decorator that bounds each provider call with a configured deadline.
pub struct TimedRateLimiter<R, D> {
    inner: R,
    deadlines: D,
    timeout: Duration,
}

impl<R, D> TimedRateLimiter<R, D> {
    /// Wraps a provider with a runtime timer and a bounded, nonzero timeout.
    pub fn new(
        inner: R,
        deadlines: D,
        timeout: Duration,
    ) -> Result<Self, ProviderTimeoutConfigurationError> {
        validate_provider_timeout(timeout)?;
        Ok(Self {
            inner,
            deadlines,
            timeout,
        })
    }
}

pub(crate) fn validate_provider_timeout(
    timeout: Duration,
) -> Result<(), ProviderTimeoutConfigurationError> {
    if timeout.is_zero() || timeout.as_nanos() > u128::from(u64::MAX) {
        Err(ProviderTimeoutConfigurationError)
    } else {
        Ok(())
    }
}

impl<R, D> RateLimiter for TimedRateLimiter<R, D>
where
    R: RateLimiter,
    D: DeadlineFactory,
{
    fn check_and_consume<'a>(
        &'a self,
        request: RateLimitRequest<'a>,
    ) -> BoxFuture<'a, Result<RateLimitDecision, RateLimitProviderError>> {
        race_provider(
            self.inner.check_and_consume(request),
            &self.deadlines,
            self.timeout,
        )
    }
}

pub(crate) fn check_with_request_deadline<'a>(
    limiter: &'a dyn RateLimiter,
    request: RateLimitRequest<'a>,
    deadlines: Option<&'a dyn DeadlineFactory>,
    timeout: Option<Duration>,
) -> BoxFuture<'a, Result<RateLimitDecision, CheckedProviderError>> {
    let provider = match (deadlines, timeout) {
        (_, None) => limiter.check_and_consume(request),
        (Some(deadlines), Some(timeout)) => {
            race_provider(limiter.check_and_consume(request), deadlines, timeout)
        }
        (None, Some(_)) => {
            Box::pin(async { Err(RateLimitProviderError::new(MissingProviderDeadline)) })
        }
    };
    Box::pin(async move {
        let decision = provider.await.map_err(CheckedProviderError::Provider)?;
        decision.validate_for(request).map_err(|error| {
            CheckedProviderError::InvalidDecision(RateLimitProviderError::new(error))
        })
    })
}

pub(crate) enum CheckedProviderError {
    Provider(RateLimitProviderError),
    InvalidDecision(RateLimitProviderError),
}

impl CheckedProviderError {
    pub(crate) const fn source(&self) -> &RateLimitProviderError {
        match self {
            Self::Provider(error) | Self::InvalidDecision(error) => error,
        }
    }

    pub(crate) const fn is_invalid_decision(&self) -> bool {
        matches!(self, Self::InvalidDecision(_))
    }
}

fn race_provider<'a>(
    mut target: BoxFuture<'a, Result<RateLimitDecision, RateLimitProviderError>>,
    deadlines: &'a dyn DeadlineFactory,
    duration: Duration,
) -> BoxFuture<'a, Result<RateLimitDecision, RateLimitProviderError>> {
    let mut timeout = deadlines.sleep(duration);
    Box::pin(poll_fn(move |context| {
        if timeout.as_mut().poll(context).is_ready() {
            return Poll::Ready(Err(RateLimitProviderError::new(ProviderTimeout)));
        }
        target.as_mut().poll(context)
    }))
}

#[derive(Debug)]
struct ProviderTimeout;

impl std::fmt::Display for ProviderTimeout {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("rate provider timed out")
    }
}

impl std::error::Error for ProviderTimeout {}

#[derive(Debug)]
struct MissingProviderDeadline;

impl std::fmt::Display for MissingProviderDeadline {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("rate provider deadline source is unavailable")
    }
}

impl std::error::Error for MissingProviderDeadline {}
