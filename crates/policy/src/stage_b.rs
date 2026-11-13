//! Cost and quota plugin that runs before resolver execution.

use std::sync::Arc;

use chigiri_runtime::{
    ErrorEvent, ErrorObserver, GraphQlError, GraphQlResponse, LifecyclePhase, OperationContext,
    OperationNext, Plugin, RequestIdentity,
};

use crate::{
    OperationCost, OperationCostContext, OperationCostPolicy, ProviderFailurePolicy,
    ProviderTimeoutConfigurationError, QuotaKey, RateLimitPolicyError, RateLimitRequest,
    RateLimitSnapshot, RateLimiter, RatePolicy,
    observer_binding::ObserverSlot,
    provider_timeout::{check_with_request_deadline, validate_provider_timeout},
};

/// To extract the bounded quota key from application context and operation identity.
pub trait OperationQuotaExtractor<C>: Send + Sync + 'static {
    fn extract(&self, context: &OperationContext<'_, C>) -> Result<QuotaKey, GraphQlError>;
}

/// Connects the checked cost cap and Stage B atomic consumption to the
/// operation plugin.
pub struct OperationRateLimit<E> {
    extractor: E,
    cost_policy: Arc<dyn OperationCostPolicy>,
    limiter: Arc<dyn RateLimiter>,
    rate_policy: RatePolicy,
    failure_policy: ProviderFailurePolicy,
    max_cost: OperationCost,
    error_observer: ObserverSlot,
    provider_timeout: Option<std::time::Duration>,
}

impl<E> OperationRateLimit<E> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        extractor: E,
        cost_policy: impl OperationCostPolicy,
        limiter: impl RateLimiter,
        rate_policy: RatePolicy,
        failure_policy: ProviderFailurePolicy,
        max_cost: OperationCost,
    ) -> Result<Self, RateLimitPolicyError> {
        if max_cost.get() > rate_policy.limit() {
            return Err(RateLimitPolicyError);
        }
        Ok(Self {
            extractor,
            cost_policy: Arc::new(cost_policy),
            limiter: Arc::new(limiter),
            rate_policy,
            failure_policy,
            max_cost,
            error_observer: ObserverSlot::default(),
            provider_timeout: None,
        })
    }

    /// Sets an observer that reports provider failures independently of request results.
    pub fn with_error_observer(self, observer: impl ErrorObserver) -> Self {
        self.error_observer.set(Some(Arc::new(observer)));
        self
    }

    /// Bound the provider future with the request timer.
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
        identity: &chigiri_runtime::OperationIdentity,
        telemetry: Option<&chigiri_runtime::RequestTelemetry>,
    ) {
        let Some(observer) = self.error_observer.get() else {
            return;
        };
        let request_identity = telemetry.map_or_else(
            || RequestIdentity::new(None, None, Some(identity)),
            |telemetry| telemetry.identity(Some(identity)),
        );
        observer.observe_error(ErrorEvent::new(
            None,
            request_identity,
            LifecyclePhase::OperationAdmission,
            "RATE_LIMIT_PROVIDER_FAILED",
            error,
            None,
            None,
            "stage-b-rate-limiter",
        ));
    }
}

impl<E: Send + Sync + 'static> crate::RateLimitObserverBinding for OperationRateLimit<E> {
    fn set_error_observer(&self, observer: Option<Arc<dyn ErrorObserver>>) {
        self.error_observer.set(observer);
    }
}

impl<C, E> Plugin<C> for OperationRateLimit<E>
where
    C: Send + Sync + 'static,
    E: OperationQuotaExtractor<C>,
{
    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, C>,
        next: OperationNext<'a, C>,
    ) -> chigiri_runtime::core::executor::BoxFuture<'a, GraphQlResponse> {
        Box::pin(async move {
            let Some(identity) = context.identity() else {
                return cost_error();
            };
            let complexity = context.complexity();
            let Ok(expanded_fields) = u64::try_from(complexity.expanded_fields) else {
                return cost_error();
            };
            let Ok(aliases) = u64::try_from(complexity.aliases) else {
                return cost_error();
            };
            let Ok(depth) = u64::try_from(complexity.depth) else {
                return cost_error();
            };
            let catalog_override = match context.catalog_cost_hint() {
                Some(value) => match OperationCost::new(value) {
                    Ok(cost) => Some(cost),
                    Err(_) => return cost_error(),
                },
                None => None,
            };
            let view = OperationCostContext::new(
                identity,
                expanded_fields,
                aliases,
                depth,
                catalog_override,
                context.cost_fields(),
            );
            let Ok(cost) = self.cost_policy.cost(view) else {
                return cost_error();
            };
            if cost > self.max_cost {
                return cost_error();
            }
            let key = match self.extractor.extract(&context) {
                Ok(key) => key,
                Err(error) => return GraphQlResponse::request_error(error),
            };
            match check_with_request_deadline(
                self.limiter.as_ref(),
                RateLimitRequest::new(&key, cost, self.rate_policy),
                context.deadline_factory(),
                self.provider_timeout,
            )
            .await
            {
                Ok(decision) if decision.is_allowed() => next.run(context).await,
                Ok(decision) => rate_error(decision.snapshot()),
                Err(error)
                    if !error.is_invalid_decision()
                        && self.failure_policy == ProviderFailurePolicy::FailOpen =>
                {
                    self.observe_provider_failure(
                        error.source(),
                        identity,
                        context.request_telemetry(),
                    );
                    next.run(context).await
                }
                Err(error) => {
                    self.observe_provider_failure(
                        error.source(),
                        identity,
                        context.request_telemetry(),
                    );
                    unavailable_error()
                }
            }
        })
    }
}

fn cost_error() -> GraphQlResponse {
    GraphQlResponse::request_error(
        GraphQlError::new("Operation cost exceeds the configured budget")
            .code("OPERATION_COST_EXCEEDED"),
    )
}

fn rate_error(snapshot: RateLimitSnapshot) -> GraphQlResponse {
    let mut error = GraphQlError::new("Rate limit exceeded").code("RATE_LIMITED");
    if let Some(retry_after) = snapshot.retry_after() {
        let seconds = retry_after
            .as_secs()
            .saturating_add(u64::from(retry_after.subsec_nanos() > 0));
        error = error.extension("retryAfterSeconds", seconds);
    }
    GraphQlResponse::request_error(error)
}

fn unavailable_error() -> GraphQlResponse {
    GraphQlResponse::request_error(
        GraphQlError::new("Rate limit service is unavailable").code("RATE_LIMIT_UNAVAILABLE"),
    )
}
