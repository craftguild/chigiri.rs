//! Explicit schema policy configuration and validated assembly.

use std::marker::PhantomData;
use std::sync::Arc;

use chigiri_core::ExecutionLimits;
use chigiri_core::executable::CustomScalarRegistry;
use chigiri_core::executor::{Plugin, ResolverDispatch, generated_resolver_abi_fingerprint};

use crate::diagnostics::{Diagnostics, FieldDiagnosticsPlugin, request_identity};
use crate::{GENERATED_ABI_VERSION, SchemaDescriptor, SchemaInfo, cache, runtime_schema};

use super::{
    DocumentCacheLimits, ExecutableSchema, ExecutableSchemaBuilder, IntrospectionPolicy,
    SchemaBuildError, SchemaInner,
};

impl<C> ExecutableSchemaBuilder<C>
where
    C: Send + Sync + 'static,
{
    pub(super) fn new(
        descriptor: &'static SchemaDescriptor,
        resolvers: Arc<dyn ResolverDispatch<C>>,
    ) -> Self {
        Self {
            descriptor,
            resolvers,
            subscription: None,
            limits: ExecutionLimits::production(),
            introspection: IntrospectionPolicy::Disabled,
            cache_limits: DocumentCacheLimits::default(),
            plugins: Vec::new(),
            diagnostics: Diagnostics::default(),
            catalog_revision: None,
            expected_resolver_abi: None,
            marker: PhantomData,
        }
    }

    /// Replaces all direct-execution resource limits.
    pub const fn limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Replaces the schema introspection policy.
    pub const fn introspection(mut self, policy: IntrospectionPolicy) -> Self {
        self.introspection = policy;
        self
    }

    /// Replaces the validated-document cache capacity.
    pub const fn document_cache(mut self, limits: DocumentCacheLimits) -> Self {
        self.cache_limits = limits;
        self
    }

    /// Add the plugin to the end of the registration sequence.
    pub fn plugin<P>(mut self, plugin: P) -> Self
    where
        P: Plugin<C>,
    {
        self.plugins.push(Arc::new(plugin));
        self
    }

    /// Sets the factory for opaque internal-error IDs.
    pub fn error_id_factory(mut self, factory: impl chigiri_core::ErrorIdFactory) -> Self {
        self.diagnostics = self.diagnostics.with_error_ids(factory);
        self
    }

    /// Sets the factory for opaque request IDs.
    pub fn request_id_factory(mut self, factory: impl chigiri_core::RequestIdFactory) -> Self {
        self.diagnostics = self.diagnostics.with_request_ids(factory);
        self
    }

    /// Sets the sampler called once when each request starts.
    pub fn sampler(mut self, sampler: impl chigiri_core::Sampler) -> Self {
        self.diagnostics = self.diagnostics.with_sampler(sampler);
        self
    }

    /// Replaces the optional owned-execution instrumentation provider.
    pub fn instrumentation(
        mut self,
        provider: impl crate::instrumentation::Instrumentation,
    ) -> Self {
        self.diagnostics = self.diagnostics.with_instrumentation(provider);
        self
    }

    /// Sets the lifecycle-event observer.
    pub fn lifecycle_observer(mut self, observer: impl chigiri_core::LifecycleObserver) -> Self {
        self.diagnostics = self.diagnostics.with_lifecycle(observer);
        self
    }

    /// Sets the internal-error source observer.
    pub fn error_observer(mut self, observer: impl chigiri_core::ErrorObserver) -> Self {
        self.diagnostics = self.diagnostics.with_errors(observer);
        self
    }

    /// Enables or disables instrumentation for successful field resolution.
    pub fn field_instrumentation(mut self, enabled: bool) -> Self {
        self.diagnostics = self.diagnostics.with_field_instrumentation(enabled);
        self
    }

    /// Sets the validated operation-catalog revision exposed in readiness metadata.
    pub fn catalog_revision(mut self, revision: impl Into<Arc<str>>) -> Self {
        self.catalog_revision = Some(revision.into());
        self
    }

    /// Sets the ABI fingerprint shared by the complete generated source set.
    #[doc(hidden)]
    pub const fn resolver_abi_fingerprint(
        mut self,
        fingerprint: chigiri_core::executor::ResolverSchemaFingerprint,
    ) -> Self {
        self.expected_resolver_abi = Some(fingerprint);
        self
    }

    /// Validates ABI and descriptor compatibility before creating the schema handle.
    pub fn build(mut self) -> Result<ExecutableSchema<C>, SchemaBuildError> {
        let build_started = std::time::Instant::now();
        if self.catalog_revision.as_ref().is_some_and(|revision| {
            revision.is_empty()
                || revision.len() > 128
                || !revision.bytes().all(|byte| matches!(byte, 0x21..=0x7e))
        }) {
            return Err(SchemaBuildError::new(
                "catalog revision must contain 1 to 128 visible ASCII bytes",
            ));
        }
        if self.descriptor.abi_version != GENERATED_ABI_VERSION {
            return Err(SchemaBuildError::new(format!(
                "generated ABI mismatch: expected {GENERATED_ABI_VERSION}, got {}",
                self.descriptor.abi_version
            )));
        }
        let semantic_schema = runtime_schema::build(self.descriptor).map_err(|message| {
            SchemaBuildError::new(format!("generated schema descriptor is invalid: {message}"))
        })?;
        let expected = generated_resolver_abi_fingerprint(&semantic_schema);
        if self.resolvers.schema_fingerprint() != expected {
            return Err(SchemaBuildError::new(
                "generated schema descriptor and resolver dispatch table do not match",
            ));
        }
        if self
            .expected_resolver_abi
            .is_some_and(|expected| self.resolvers.resolver_abi_fingerprint() != expected)
        {
            return Err(SchemaBuildError::new(
                "generated resolver ABI fingerprint does not match its wrapper",
            ));
        }
        if self.descriptor.roots.subscription
            != self
                .subscription
                .as_ref()
                .and_then(|dispatch| dispatch.subscription_root())
        {
            return Err(SchemaBuildError::new(
                "generated descriptor and subscription dispatch root do not match",
            ));
        }
        let mut scalars = CustomScalarRegistry::new();
        self.resolvers.register_custom_scalars(&mut scalars);
        if self.diagnostics.field_instrumentation() {
            self.plugins.insert(
                0,
                Arc::new(FieldDiagnosticsPlugin::new(self.diagnostics.clone())),
            );
        }
        let schema = ExecutableSchema {
            inner: Arc::new(SchemaInner {
                info: SchemaInfo::generated(
                    env!("CARGO_PKG_VERSION"),
                    GENERATED_ABI_VERSION,
                    self.descriptor.schema_digest,
                )
                .with_optional_catalog_revision(self.catalog_revision),
                semantic_schema: Arc::new(semantic_schema),
                resolvers: Arc::new(crate::dispatch::GeneratedDispatch(self.resolvers)),
                subscription: self.subscription,
                scalars: Arc::new(scalars),
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
        Ok(schema)
    }
}
