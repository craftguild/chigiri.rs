//! Defining the provider fixture to share in the Rate Limit Acceptance Test.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri::rate::{OperationQuotaExtractor, TransportQuotaExtractor};

pub(crate) fn transport_builder() -> chigiri::rate::AlgorithmBuilder<
    chigiri::rate::Transport,
    TransportKey,
    chigiri::rate::TokenBucket,
> {
    chigiri::rate::transport(TransportKey)
        .token_bucket()
        .capacity(10)
        .unwrap()
        .refill(10, Duration::from_secs(60))
        .unwrap()
}

pub(crate) struct TransportKey;

impl TransportQuotaExtractor for TransportKey {
    fn extract(
        &self,
        _: chigiri::TransportRequest<'_>,
    ) -> Result<chigiri::rate::QuotaKey, chigiri::spi::AdmissionError> {
        Ok(key("transport"))
    }
}

pub(crate) struct OperationKey;

impl OperationQuotaExtractor<()> for OperationKey {
    fn extract(
        &self,
        _: &chigiri::OperationContext<'_, ()>,
    ) -> Result<chigiri::rate::QuotaKey, chigiri::GraphQlError> {
        Ok(key("operation"))
    }
}

pub(crate) fn key(value: &str) -> chigiri::rate::QuotaKey {
    chigiri::rate::QuotaKey::new(
        value.as_bytes().to_vec(),
        chigiri::rate::QuotaSubject::Missing,
        None,
    )
    .unwrap()
}

pub(crate) fn rate_policy(limit: u64) -> chigiri::rate::RatePolicy {
    chigiri::rate::TokenBucketPolicy::new(limit, limit, Duration::from_secs(60))
        .unwrap()
        .into()
}

pub(crate) fn code(response: &chigiri::GraphQlResponse) -> Option<&str> {
    response.errors.first()?.extensions.get("code")?.as_str()
}

pub(crate) struct CountingLimiter {
    calls: Arc<AtomicUsize>,
    reject: bool,
}

impl CountingLimiter {
    pub(crate) fn allow(calls: Arc<AtomicUsize>) -> Self {
        Self {
            calls,
            reject: false,
        }
    }

    pub(crate) fn reject(calls: Arc<AtomicUsize>) -> Self {
        Self {
            calls,
            reject: true,
        }
    }
}

impl chigiri::rate::RateLimiter for CountingLimiter {
    fn check_and_consume<'a>(
        &'a self,
        request: chigiri::rate::RateLimitRequest<'a>,
    ) -> chigiri::BoxFuture<
        'a,
        Result<chigiri::rate::RateLimitDecision, chigiri::rate::RateLimitProviderError>,
    > {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let policy = request.policy();
        let decision = if self.reject {
            chigiri::rate::RateLimitDecision::rejected(
                request,
                0,
                chigiri::rate::MonotonicTick::from_nanos(60_000_000_000),
                Some(Duration::from_secs(1)),
                chigiri::rate::RateLimitRejectReason::QuotaExceeded,
            )
            .unwrap()
        } else {
            chigiri::rate::RateLimitDecision::allowed(
                request,
                policy.limit() - 1,
                chigiri::rate::MonotonicTick::from_nanos(60_000_000_000),
            )
            .unwrap()
        };
        Box::pin(async move { Ok(decision) })
    }
}

pub(crate) struct PendingLimiter(pub(crate) Arc<AtomicBool>);

impl chigiri::rate::RateLimiter for PendingLimiter {
    fn check_and_consume<'a>(
        &'a self,
        _: chigiri::rate::RateLimitRequest<'a>,
    ) -> chigiri::BoxFuture<
        'a,
        Result<chigiri::rate::RateLimitDecision, chigiri::rate::RateLimitProviderError>,
    > {
        Box::pin(Pending(Arc::clone(&self.0)))
    }
}

struct Pending(Arc<AtomicBool>);

impl Future for Pending {
    type Output = Result<chigiri::rate::RateLimitDecision, chigiri::rate::RateLimitProviderError>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

pub(crate) struct ImmediateDeadline;

impl chigiri::DeadlineFactory for ImmediateDeadline {
    fn sleep(&self, _: Duration) -> chigiri::BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

pub(crate) struct ProviderDeadline;

impl chigiri::DeadlineFactory for ProviderDeadline {
    fn sleep(&self, duration: Duration) -> chigiri::BoxFuture<'_, ()> {
        if duration == Duration::from_millis(1) {
            Box::pin(async {})
        } else {
            Box::pin(std::future::pending())
        }
    }
}

pub(crate) struct BypassOperation;

impl chigiri::Plugin<()> for BypassOperation {
    fn operation<'a>(
        &'a self,
        _: chigiri::OperationContext<'a, ()>,
        _: chigiri::OperationNext<'a, ()>,
    ) -> chigiri::BoxFuture<'a, chigiri::GraphQlResponse> {
        Box::pin(async {
            chigiri::GraphQlResponse::request_error(
                chigiri::GraphQlError::new("bypassed").code("BYPASSED"),
            )
        })
    }
}
