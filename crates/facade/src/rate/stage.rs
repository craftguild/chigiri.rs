//! Preserve the stage kind in the type system to prevent injection into the wrong slot.

use std::time::Duration;

use chigiri_http::{AdmissionError, BoxFuture, TransportAdmission, TransportRequest};
use chigiri_policy::{ProviderFailurePolicy, RateLimitObserverBinding};
use chigiri_rate::RatePolicy;
use chigiri_runtime::__private::ResolvedValue;
use chigiri_runtime::{
    FieldContext, FieldNext, FieldResult, GraphQlResponse, OperationContext, OperationNext, Plugin,
    RequestContext, RequestNext,
};

use crate::components::Components;
use crate::configuration::RateLimitStageConfiguration;
use crate::draft::Draft;

/// Stage A wrapper with transport admission and redacted rate settings.
pub struct TransportRateStage<A> {
    inner: A,
    provider_timeout: Duration,
    failure_policy: ProviderFailurePolicy,
    policy: RatePolicy,
}

impl<A> TransportRateStage<A> {
    pub(crate) const fn new(
        inner: A,
        provider_timeout: Duration,
        failure_policy: ProviderFailurePolicy,
        policy: RatePolicy,
    ) -> Self {
        Self {
            inner,
            provider_timeout,
            failure_policy,
            policy,
        }
    }
    /// Provider-decision deadline.
    pub const fn provider_timeout(&self) -> Duration {
        self.provider_timeout
    }
    /// Policy applied after a provider failure.
    pub const fn failure_policy(&self) -> ProviderFailurePolicy {
        self.failure_policy
    }
    /// Return the quota policy for Stage A.
    pub const fn policy(&self) -> RatePolicy {
        self.policy
    }
}

impl<A: TransportAdmission> TransportAdmission for TransportRateStage<A> {
    fn admit<'a>(
        &'a self,
        request: TransportRequest<'a>,
    ) -> BoxFuture<'a, Result<chigiri_http::AdmissionDecision, AdmissionError>> {
        self.inner.admit(request)
    }
}

/// Contract through which the typed component graph also receives Stage A metadata.
pub trait TransportRateComponent: TransportAdmission + RateLimitObserverBinding {
    /// Returns redacted metadata for the effective configuration.
    fn configuration(&self) -> RateLimitStageConfiguration;
}

impl<A: TransportAdmission + RateLimitObserverBinding> TransportRateComponent
    for TransportRateStage<A>
{
    fn configuration(&self) -> RateLimitStageConfiguration {
        RateLimitStageConfiguration::new(self.provider_timeout, self.failure_policy, self.policy)
    }
}

/// Stage B wrapper with an operation plugin and redacted rate configuration.
pub struct OperationRateStage<P> {
    inner: P,
    provider_timeout: Duration,
    failure_policy: ProviderFailurePolicy,
    policy: RatePolicy,
}

impl<P> OperationRateStage<P> {
    pub(crate) const fn new(
        inner: P,
        provider_timeout: Duration,
        failure_policy: ProviderFailurePolicy,
        policy: RatePolicy,
    ) -> Self {
        Self {
            inner,
            provider_timeout,
            failure_policy,
            policy,
        }
    }
    /// Provider-decision deadline.
    pub const fn provider_timeout(&self) -> Duration {
        self.provider_timeout
    }
    /// Policy applied after a provider failure.
    pub const fn failure_policy(&self) -> ProviderFailurePolicy {
        self.failure_policy
    }
    /// Return the quota policy for Stage B.
    pub const fn policy(&self) -> RatePolicy {
        self.policy
    }
}

impl<C: 'static, P: Plugin<C>> Plugin<C> for OperationRateStage<P> {
    fn request<'a>(
        &'a self,
        context: RequestContext<'a, C>,
        next: RequestNext<'a, C>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        self.inner.request(context, next)
    }
    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, C>,
        next: OperationNext<'a, C>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        self.inner.operation(context, next)
    }
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, C>,
        next: FieldNext<'a, C>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.inner.field(context, next)
    }
}

/// Contract through which the typed component graph also receives Stage B metadata.
pub trait OperationRateComponent<C: 'static>: Plugin<C> + RateLimitObserverBinding {
    /// Returns redacted metadata for the effective configuration.
    fn configuration(&self) -> RateLimitStageConfiguration;
}

impl<C: 'static, P: Plugin<C> + RateLimitObserverBinding> OperationRateComponent<C>
    for OperationRateStage<P>
{
    fn configuration(&self) -> RateLimitStageConfiguration {
        RateLimitStageConfiguration::new(self.provider_timeout, self.failure_policy, self.policy)
    }
}

impl<A: RateLimitObserverBinding> RateLimitObserverBinding for TransportRateStage<A> {
    fn set_error_observer(
        &self,
        observer: Option<std::sync::Arc<dyn chigiri_runtime::ErrorObserver>>,
    ) {
        self.inner.set_error_observer(observer);
    }
}

impl<P: RateLimitObserverBinding> RateLimitObserverBinding for OperationRateStage<P> {
    fn set_error_observer(
        &self,
        observer: Option<std::sync::Arc<dyn chigiri_runtime::ErrorObserver>>,
    ) {
        self.inner.set_error_observer(observer);
    }
}

mod sealed {
    use super::*;

    pub trait Apply<C> {
        fn apply(self, components: &mut Components<'_, C>);
    }

    impl<C, A> Apply<C> for TransportRateStage<A>
    where
        C: Send + Sync + 'static,
        A: TransportAdmission + RateLimitObserverBinding,
    {
        fn apply(self, components: &mut Components<'_, C>) {
            components.rate_limit().stage_a(self);
        }
    }

    impl<C, P> Apply<C> for OperationRateStage<P>
    where
        C: Send + Sync + 'static,
        P: Plugin<C> + RateLimitObserverBinding,
    {
        fn apply(self, components: &mut Components<'_, C>) {
            components.rate_limit().stage_b(self);
        }
    }
}

/// Sealed builder contract that installs Stage A and Stage B in their correct slots.
pub trait RateStage<C>: sealed::Apply<C> {}

impl<C, A> RateStage<C> for TransportRateStage<A>
where
    C: Send + Sync + 'static,
    A: TransportAdmission + RateLimitObserverBinding,
{
}

impl<C, P> RateStage<C> for OperationRateStage<P>
where
    C: Send + Sync + 'static,
    P: Plugin<C> + RateLimitObserverBinding,
{
}

pub(crate) fn apply_stage<C, S>(stage: S, draft: &mut Draft<C>)
where
    S: RateStage<C>,
{
    sealed::Apply::apply(
        stage,
        &mut Components {
            draft,
            response_state: std::marker::PhantomData,
        },
    );
}
