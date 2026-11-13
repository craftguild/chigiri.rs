//! Typed injection API for editing a draft graph through closed namespaces.

use std::sync::Arc;

use chigiri_http::OperationSourceResolver;
use chigiri_runtime::{
    DocumentCacheLimits, ErrorIdFactory, ErrorObserver, ExecutionLimits, LifecycleObserver, Plugin,
    RequestIdFactory, Sampler,
};

use crate::component_ops;
use crate::draft::Draft;

mod http;
mod rate;
mod subscriptions;
pub use http::HttpComponents;
pub use rate::RateLimitComponents;
pub use subscriptions::SubscriptionComponents;

/// The editor borrows one component graph only during `inject` closure.
pub struct Components<'a, C, S = ()> {
    pub(crate) draft: &'a mut Draft<C>,
    pub(crate) response_state: std::marker::PhantomData<fn() -> S>,
}

impl<C, S> Components<'_, C, S>
where
    C: Send + Sync + 'static,
{
    /// Borrows the execution-settings namespace.
    pub fn execution(&mut self) -> ExecutionComponents<'_, C> {
        ExecutionComponents { draft: self.draft }
    }
    /// Borrows the HTTP-boundary settings namespace.
    pub fn http(&mut self) -> HttpComponents<'_, C, S> {
        HttpComponents {
            draft: self.draft,
            response_state: std::marker::PhantomData,
        }
    }
    /// Borrows the operation-source settings namespace.
    pub fn operation(&mut self) -> OperationComponents<'_, C> {
        OperationComponents { draft: self.draft }
    }
    /// Borrows the rate-limit settings namespace.
    pub fn rate_limit(&mut self) -> RateLimitComponents<'_, C> {
        RateLimitComponents { draft: self.draft }
    }
    /// Borrows the subscription settings namespace.
    pub fn subscriptions(&mut self) -> SubscriptionComponents<'_, C> {
        SubscriptionComponents { draft: self.draft }
    }
    /// Borrows the observability settings namespace.
    pub fn observability(&mut self) -> ObservabilityComponents<'_, C> {
        ObservabilityComponents { draft: self.draft }
    }
    /// Borrows the plugin-collection settings namespace.
    pub fn plugins(&mut self) -> PluginComponents<'_, C> {
        PluginComponents { draft: self.draft }
    }
}

/// Execution settings applied across parsing, validation, completion, and conversion.
pub struct ExecutionComponents<'a, C> {
    draft: &'a mut Draft<C>,
}

impl<C> ExecutionComponents<'_, C>
where
    C: Send + Sync + 'static,
{
    /// Sets structural limits from parsing through response completion.
    pub fn limits(&mut self, limits: ExecutionLimits) -> &mut Self {
        component_ops::execution_limits(self.draft, limits);
        self
    }

    /// Sets document-cache entry and byte limits.
    pub fn document_cache(&mut self, limits: DocumentCacheLimits) -> &mut Self {
        component_ops::document_cache(self.draft, limits);
        self
    }

    /// Explicitly enables or disables schema introspection with an audit reason.
    pub fn dangerously_enable_introspection(&mut self, reason: impl Into<Arc<str>>) -> &mut Self {
        Draft::<C>::set(
            &mut self.draft.sequence,
            &mut self.draft.introspection,
            true,
        );
        self.draft
            .add_override("schema-introspection", reason.into());
        self
    }
}

/// Set the raw/persisted operation source.
pub struct OperationComponents<'a, C> {
    draft: &'a mut Draft<C>,
}

impl<C> OperationComponents<'_, C>
where
    C: Send + Sync + 'static,
{
    /// Set the raw or persisted operation solver.
    pub fn source(&mut self, source: impl OperationSourceResolver) -> &mut Self {
        component_ops::operation_source(self.draft, source);
        self
    }

    /// Restores raw request documents as the operation source.
    pub fn raw_document_source(&mut self) -> &mut Self {
        self.draft.mark_change();
        self.draft.source = None;
        self
    }
}

/// Configures identifiers, sampling, observers, and field instrumentation.
pub struct ObservabilityComponents<'a, C> {
    draft: &'a mut Draft<C>,
}

impl<C> ObservabilityComponents<'_, C>
where
    C: Send + Sync + 'static,
{
    /// Sets the error-ID factory.
    pub fn error_id_factory(&mut self, factory: impl ErrorIdFactory) -> &mut Self {
        self.draft.mark_change();
        self.draft.error_ids = Arc::new(factory);
        self
    }

    /// Sets the request-ID factory.
    ///
    /// In a prefork server, create the factory after the worker fork and build the
    /// Build the endpoint in that same worker.
    pub fn request_id_factory(&mut self, factory: impl RequestIdFactory) -> &mut Self {
        self.draft.mark_change();
        self.draft.request_ids = crate::draft::RequestIdPlan::Configured(Arc::new(factory));
        self
    }

    /// Sets the trace sampler.
    pub fn sampler(&mut self, sampler: impl Sampler) -> &mut Self {
        self.draft.mark_change();
        self.draft.sampler = Some(Arc::new(sampler));
        self
    }

    /// Replaces one optional execution-instrumentation provider.
    ///
    /// This leaves lifecycle/error observer slots independently configured.
    /// The application continues to own telemetry initialization and shutdown.
    pub fn instrumentation(
        &mut self,
        provider: impl chigiri_runtime::instrumentation::Instrumentation,
    ) -> &mut Self {
        self.draft.mark_change();
        self.draft.instrumentation = Some(Arc::new(provider));
        self
    }

    /// Set the lifecycle observer to a single slot.
    pub fn lifecycle_observer(&mut self, observer: impl LifecycleObserver) -> &mut Self {
        self.draft.mark_change();
        self.draft.lifecycle_observer = Some(Arc::new(observer));
        self
    }

    /// Propagates the final error observer to every component.
    pub fn error_observer(&mut self, observer: impl ErrorObserver) -> &mut Self {
        self.draft.mark_change();
        let observer: Arc<dyn ErrorObserver> = Arc::new(observer);
        self.draft.error_observer = Some(Arc::clone(&observer));
        if let Some(binding) = &self.draft.stage_a_observer {
            binding.set_error_observer(Some(Arc::clone(&observer)));
        }
        if let Some(binding) = &self.draft.stage_b_observer {
            binding.set_error_observer(Some(observer));
        }
        self
    }

    /// Enable or disable per-field instrumentation.
    pub fn field_instrumentation(&mut self, enabled: bool) -> &mut Self {
        self.draft.mark_change();
        self.draft.field_instrumentation = enabled;
        self
    }
}

/// Set up the request/operation/field plugin collection.
pub struct PluginComponents<'a, C> {
    draft: &'a mut Draft<C>,
}

impl<C> PluginComponents<'_, C>
where
    C: Send + Sync + 'static,
{
    /// Appends a plugin to the execution order.
    pub fn push_plugin(&mut self, plugin: impl Plugin<C>) -> &mut Self {
        component_ops::push_plugin(self.draft, plugin);
        self
    }

    /// Replaces the entire plugin collection.
    pub fn set_plugins(&mut self, plugins: Vec<Arc<dyn Plugin<C>>>) -> &mut Self {
        self.draft.mark_change();
        self.draft.plugins = plugins;
        self
    }

    /// Clears the plugin collection.
    pub fn clear_plugins(&mut self) -> &mut Self {
        self.draft.mark_change();
        self.draft.plugins.clear();
        self
    }
}
