//! Transport rate-limit bridge that runs before body polling.

use std::sync::Arc;

use chigiri_http::{
    AdmissionDecision, AdmissionError, BoxFuture, Rejection, RetryAfter, TransportAdmission,
    TransportRequest,
};
use chigiri_runtime::{ErrorEvent, ErrorObserver, GraphQlError, LifecyclePhase, RequestIdentity};

use crate::{
    OperationCost, ProviderFailurePolicy, ProviderTimeoutConfigurationError, QuotaKey,
    RateLimitRequest, RateLimitSnapshot, RateLimiter, RatePolicy,
    observer_binding::ObserverSlot,
    provider_timeout::{check_with_request_deadline, validate_provider_timeout},
};

/// Extracts a bounded quota key from transport-neutral request metadata.
pub trait TransportQuotaExtractor: Send + Sync + 'static {
    fn extract(&self, request: TransportRequest<'_>) -> Result<QuotaKey, AdmissionError>;
}

/// Connects Stage A's atomic consumption to transport admission.
pub struct StageARateLimit<E> {
    extractor: E,
    limiter: Arc<dyn RateLimiter>,
    policy: RatePolicy,
    failure_policy: ProviderFailurePolicy,
    error_observer: ObserverSlot,
    provider_timeout: Option<std::time::Duration>,
}

impl<E> StageARateLimit<E> {
    pub fn new(
        extractor: E,
        limiter: impl RateLimiter,
        policy: RatePolicy,
        failure_policy: ProviderFailurePolicy,
    ) -> Self {
        Self {
            extractor,
            limiter: Arc::new(limiter),
            policy,
            failure_policy,
            error_observer: ObserverSlot::default(),
            provider_timeout: None,
        }
    }

    /// Sets the observer that receives provider failures independently of request results.
    pub fn with_error_observer(self, observer: impl ErrorObserver) -> Self {
        self.error_observer.set(Some(Arc::new(observer)));
        self
    }

    /// Bounds the provider future with the request deadline factory.
    pub fn with_provider_timeout(
        mut self,
        timeout: std::time::Duration,
    ) -> Result<Self, ProviderTimeoutConfigurationError> {
        validate_provider_timeout(timeout)?;
        self.provider_timeout = Some(timeout);
        Ok(self)
    }

    fn observe_provider_failure(
        &self,
        error: &crate::RateLimitProviderError,
        telemetry: Option<&chigiri_runtime::RequestTelemetry>,
    ) {
        let Some(observer) = self.error_observer.get() else {
            return;
        };
        observer.observe_error(ErrorEvent::new(
            None,
            telemetry.map_or_else(
                || RequestIdentity::new(None, None, None),
                |telemetry| telemetry.identity(None),
            ),
            LifecyclePhase::TransportAdmission,
            "RATE_LIMIT_PROVIDER_FAILED",
            error,
            None,
            None,
            "stage-a-rate-limiter",
        ));
    }
}

impl<E: Send + Sync + 'static> crate::RateLimitObserverBinding for StageARateLimit<E> {
    fn set_error_observer(&self, observer: Option<Arc<dyn ErrorObserver>>) {
        self.error_observer.set(observer);
    }
}

impl<E> TransportAdmission for StageARateLimit<E>
where
    E: TransportQuotaExtractor,
{
    fn admit<'a>(
        &'a self,
        request: TransportRequest<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        Box::pin(async move {
            let telemetry = request.request_telemetry();
            let key = self.extractor.extract(request)?;
            match check_with_request_deadline(
                self.limiter.as_ref(),
                RateLimitRequest::new(&key, OperationCost::one(), self.policy),
                request.deadline_factory(),
                self.provider_timeout,
            )
            .await
            {
                Ok(decision) if decision.is_allowed() => Ok(AdmissionDecision::Continue),
                Ok(decision) => Ok(AdmissionDecision::Reject(rate_rejection(
                    decision.snapshot(),
                ))),
                Err(error)
                    if !error.is_invalid_decision()
                        && self.failure_policy == ProviderFailurePolicy::FailOpen =>
                {
                    self.observe_provider_failure(error.source(), telemetry);
                    Ok(AdmissionDecision::Continue)
                }
                Err(error) => {
                    self.observe_provider_failure(error.source(), telemetry);
                    Ok(AdmissionDecision::Reject(unavailable_rejection()))
                }
            }
        })
    }
}

fn unavailable_rejection() -> Rejection {
    Rejection::new(
        503,
        GraphQlError::new("Rate limit service is unavailable").code("RATE_LIMIT_UNAVAILABLE"),
    )
    .expect("The rate provider failure status is 503.")
}

fn rate_rejection(snapshot: RateLimitSnapshot) -> Rejection {
    let retry_seconds = snapshot.retry_after().map(duration_seconds);
    let mut error = GraphQlError::new("Rate limit exceeded").code("RATE_LIMITED");
    if let Some(seconds) = retry_seconds {
        error = error.extension("retryAfterSeconds", seconds);
    }
    let rejection = Rejection::new(429, error).expect("The rate rejection status is 429.");
    match retry_seconds {
        Some(seconds) => rejection.with_retry_after(RetryAfter::seconds(seconds)),
        None => rejection,
    }
}

fn duration_seconds(duration: std::time::Duration) -> u64 {
    duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() > 0))
}
