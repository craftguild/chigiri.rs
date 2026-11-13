//! Executable schema construction and shared request diagnostics.

use std::sync::Arc;

use chigiri_core::ExecutionLimits;
use chigiri_core::executor::ResolverDispatch;

use crate::diagnostics::request_identity;
use crate::subscription::SubscriptionResolverDispatch;
use crate::{SchemaDescriptor, SchemaInfo};

use super::{ExecutableSchema, ExecutableSchemaBuilder, IntrospectionPolicy};

impl<C> ExecutableSchema<C>
where
    C: Send + Sync + 'static,
{
    /// Creates a builder from a generated descriptor and complete resolver set.
    #[doc(hidden)]
    pub fn builder<D>(
        descriptor: &'static SchemaDescriptor,
        resolvers: D,
    ) -> ExecutableSchemaBuilder<C>
    where
        D: ResolverDispatch<C> + 'static,
    {
        ExecutableSchemaBuilder::new(descriptor, Arc::new(resolvers))
    }

    /// Creates a schema from the complete generated ordinary and subscription dispatch.
    #[doc(hidden)]
    pub fn builder_with_subscriptions<D>(
        descriptor: &'static SchemaDescriptor,
        resolvers: D,
    ) -> ExecutableSchemaBuilder<C>
    where
        D: ResolverDispatch<C> + SubscriptionResolverDispatch<C>,
    {
        let resolvers = Arc::new(resolvers);
        let subscription: Arc<dyn SubscriptionResolverDispatch<C>> = resolvers.clone();
        let ordinary: Arc<dyn ResolverDispatch<C>> = resolvers;
        let mut builder = ExecutableSchemaBuilder::new(descriptor, ordinary);
        builder.subscription = Some(subscription);
        builder
    }

    /// Returns the limits applied to request execution.
    pub fn limits(&self) -> ExecutionLimits {
        self.inner.limits
    }

    /// Returns the schema introspection policy.
    pub fn introspection_policy(&self) -> IntrospectionPolicy {
        self.inner.introspection
    }

    /// It returns the immutable readiness information of the compiled schema.
    pub fn schema_info(&self) -> &SchemaInfo {
        &self.inner.info
    }

    /// Preserves validated schema identity while applying the facade component graph.
    #[doc(hidden)]
    pub fn reconfigure(&self) -> crate::ExecutableSchemaRebuilder<C> {
        crate::ExecutableSchemaRebuilder::new(self)
    }

    /// Notifies the common observer of an HTTP lifecycle event from an adapter.
    #[doc(hidden)]
    pub fn observe_lifecycle(
        &self,
        phase: chigiri_core::LifecyclePhase,
        result: chigiri_core::LifecycleResult,
        started: Option<std::time::Instant>,
        telemetry: Option<&chigiri_core::RequestTelemetry>,
        code: Option<&'static str>,
        component: &'static str,
    ) {
        self.inner.diagnostics.observe(
            phase,
            result,
            started,
            request_identity(None, telemetry),
            code,
            component,
        );
    }

    /// Assigns an ID to an adapter internal error and notifies the observer.
    #[doc(hidden)]
    pub fn observe_internal_error(
        &self,
        phase: chigiri_core::LifecyclePhase,
        code: &'static str,
        source: &(dyn std::error::Error + Send + Sync + 'static),
        telemetry: Option<&chigiri_core::RequestTelemetry>,
        component: &'static str,
    ) -> Option<chigiri_core::ErrorId> {
        self.inner
            .diagnostics
            .observe_error(phase, code, source, None, telemetry, component)
    }

    /// Returns the immutable provider used by shared SDK capture boundaries.
    #[doc(hidden)]
    pub fn instrumentation(&self) -> Option<&dyn crate::instrumentation::Instrumentation> {
        self.inner.diagnostics.instrumentation()
    }

    /// Selects owned propagation/control before SDK task handoff or observation.
    #[doc(hidden)]
    pub fn capture_instrumentation(
        &self,
        carrier: &dyn crate::instrumentation::PropagationCarrier,
    ) -> Option<crate::instrumentation::CapturedInstrumentation> {
        self.inner.diagnostics.capture_instrumentation(carrier)
    }

    /// Binds already-budgeted subscriber metadata without reading worker context.
    #[doc(hidden)]
    pub fn instrumentation_with_values(
        &self,
        values: crate::instrumentation::PropagationContext,
    ) -> Option<crate::instrumentation::CapturedInstrumentation> {
        self.inner.diagnostics.instrumentation_with_values(values)
    }

    /// Creates metadata and execution ownership from an earlier entry snapshot.
    #[doc(hidden)]
    pub fn request_telemetry_with_capture(
        &self,
        trace_id: Option<chigiri_core::TraceId>,
        captured: Option<&crate::instrumentation::CapturedInstrumentation>,
        kind: crate::instrumentation::InstrumentationKind,
    ) -> chigiri_core::RequestTelemetry {
        self.inner
            .diagnostics
            .request_telemetry_with_capture(trace_id, captured, kind)
    }

    /// Fixes correlation and sampling once at the start of an endpoint or direct request.
    #[doc(hidden)]
    pub fn request_telemetry(
        &self,
        trace_id: Option<chigiri_core::TraceId>,
    ) -> chigiri_core::RequestTelemetry {
        self.inner.diagnostics.request_telemetry(trace_id)
    }
}
