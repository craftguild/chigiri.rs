//! Applies the facade's finalized component graph to a validated schema.

use std::sync::Arc;

use chigiri_core::ExecutionLimits;
use chigiri_core::executor::Plugin;

use crate::diagnostics::{Diagnostics, FieldDiagnosticsPlugin, request_identity};
use crate::schema::SchemaInner;
use crate::{DocumentCacheLimits, ExecutableSchema, IntrospectionPolicy, cache};

/// Builder that preserves a constructed schema's identity and resolvers while
/// reconfiguring execution components.
#[doc(hidden)]
pub struct ExecutableSchemaRebuilder<C> {
    source: ExecutableSchema<C>,
    limits: ExecutionLimits,
    introspection: IntrospectionPolicy,
    cache_limits: DocumentCacheLimits,
    plugins: Vec<Arc<dyn Plugin<C>>>,
    diagnostics: Diagnostics,
}

impl<C> ExecutableSchemaRebuilder<C>
where
    C: Send + Sync + 'static,
{
    pub(crate) fn new(source: &ExecutableSchema<C>) -> Self {
        Self {
            source: source.clone(),
            limits: source.inner.limits,
            introspection: source.inner.introspection,
            cache_limits: DocumentCacheLimits::default(),
            // The endpoint draft is the source of truth for component configuration.
            plugins: Vec::new(),
            diagnostics: Diagnostics::default(),
        }
    }

    pub const fn limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = limits;
        self
    }

    pub const fn introspection(mut self, policy: IntrospectionPolicy) -> Self {
        self.introspection = policy;
        self
    }

    pub const fn document_cache(mut self, limits: DocumentCacheLimits) -> Self {
        self.cache_limits = limits;
        self
    }

    pub fn plugin<P>(mut self, plugin: P) -> Self
    where
        P: Plugin<C>,
    {
        self.plugins.push(Arc::new(plugin));
        self
    }

    /// Prepends the higher-level facade's operation-admission plugin before
    /// existing plugins.
    pub fn prepend_plugin<P>(mut self, plugin: P) -> Self
    where
        P: Plugin<C>,
    {
        self.plugins.insert(0, Arc::new(plugin));
        self
    }

    pub fn error_id_factory(mut self, factory: impl chigiri_core::ErrorIdFactory) -> Self {
        self.diagnostics = self.diagnostics.with_error_ids(factory);
        self
    }

    pub fn request_id_factory(mut self, factory: impl chigiri_core::RequestIdFactory) -> Self {
        self.diagnostics = self.diagnostics.with_request_ids(factory);
        self
    }

    pub fn sampler(mut self, sampler: impl chigiri_core::Sampler) -> Self {
        self.diagnostics = self.diagnostics.with_sampler(sampler);
        self
    }

    pub fn lifecycle_observer(mut self, observer: impl chigiri_core::LifecycleObserver) -> Self {
        self.diagnostics = self.diagnostics.with_lifecycle(observer);
        self
    }

    /// Replaces the optional instrumentation provider in the rebuilt graph.
    pub fn instrumentation(
        mut self,
        provider: impl crate::instrumentation::Instrumentation,
    ) -> Self {
        self.diagnostics = self.diagnostics.with_instrumentation(provider);
        self
    }

    pub fn error_observer(mut self, observer: impl chigiri_core::ErrorObserver) -> Self {
        self.diagnostics = self.diagnostics.with_errors(observer);
        self
    }

    pub fn field_instrumentation(mut self, enabled: bool) -> Self {
        self.diagnostics = self.diagnostics.with_field_instrumentation(enabled);
        self
    }

    pub fn build(mut self) -> ExecutableSchema<C> {
        let build_started = std::time::Instant::now();
        if self.diagnostics.field_instrumentation() {
            self.plugins.insert(
                0,
                Arc::new(FieldDiagnosticsPlugin::new(self.diagnostics.clone())),
            );
        }
        let schema = ExecutableSchema {
            inner: Arc::new(SchemaInner {
                info: self.source.inner.info.clone(),
                semantic_schema: Arc::clone(&self.source.inner.semantic_schema),
                resolvers: Arc::clone(&self.source.inner.resolvers),
                subscription: self.source.inner.subscription.clone(),
                scalars: Arc::clone(&self.source.inner.scalars),
                limits: self.limits,
                introspection: self.introspection,
                cache: cache::DocumentCache::new(self.cache_limits),
                plugins: self.plugins,
                diagnostics: self.diagnostics,
            }),
        };
        schema.inner.diagnostics.observe(
            chigiri_core::LifecyclePhase::ServerBuilt,
            chigiri_core::LifecycleResult::Succeeded,
            Some(build_started),
            request_identity(None, None),
            None,
            "chigiri",
        );
        schema
    }
}
