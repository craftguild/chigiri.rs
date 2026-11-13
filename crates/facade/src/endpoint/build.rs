//! Final graph validation, schema configuration, and transport assembly.

use std::sync::Arc;

use chigiri_http::{ContextFactory, CsrfProtection, HttpLifecycle};
use chigiri_runtime::{ExecutableSchema, IntrospectionPolicy};

use super::{Endpoint, EndpointBuilder, validation};
use crate::EffectiveConfiguration;
use crate::build_support::{ConfigurationSlot, ConfigurationViolation, EndpointBuildError};
use crate::draft::Draft;
use crate::input::{EndpointInput, SchemaSource};
use crate::shared::{
    SharedAdmission, SharedDecoded, SharedErrorIds, SharedErrors, SharedLifecycle, SharedPlugin,
    SharedRequestIds, SharedSampler, SharedSource,
};
use crate::subscription::settings::ValidatedSubscriptionSettings;
use crate::subscription::state::SubscriptionState;
use crate::transport::{NeutralEndpoint, TransportSettings, erase_factory};

impl<C, S, F> EndpointBuilder<C, S, F>
where
    C: Send + Sync + 'static,
    S: Send + Sync + 'static,
    F: ContextFactory<C, S>,
{
    /// Validates the final graph and returns the endpoint without partial construction.
    ///
    /// In a prefork server, call this in each worker after the fork and before
    /// accepting requests. The built endpoint is process-bound and must not be
    /// transferred to a child created afterward.
    pub fn build(self) -> Result<Endpoint<C>, EndpointBuildError> {
        let parts = prepare(self.draft)?;
        let settings = transport_settings(&parts);
        let subscriptions =
            SubscriptionState::new(parts.subscriptions, parts.connection_init_authorizer).map_err(
                |_| {
                    EndpointBuildError::invalid(
                        ConfigurationSlot::WebSocketConnections,
                        parts.configuration.subscription_protocol().origin(),
                        ConfigurationViolation::ArithmeticOverflow,
                    )
                },
            )?;
        let neutral = NeutralEndpoint::new(
            parts.schema.clone(),
            parts.lifecycle,
            erase_factory(self.factory),
            parts.response_hook,
            settings,
            subscriptions,
        );
        Ok(Endpoint {
            neutral,
            schema: parts.schema,
            configuration: parts.configuration,
            #[cfg(feature = "subscription-worker")]
            managed: Arc::new(crate::managed::Binding::new()),
        })
    }
}

struct BuildParts<C> {
    schema: ExecutableSchema<C>,
    lifecycle: HttpLifecycle,
    configuration: Arc<EffectiveConfiguration>,
    body_limit: usize,
    response_limit: usize,
    body_timeout: std::time::Duration,
    transport_timeout: std::time::Duration,
    source_timeout: std::time::Duration,
    decoded_timeout: std::time::Duration,
    context_timeout: std::time::Duration,
    execution_timeout: std::time::Duration,
    response_hook_timeout: std::time::Duration,
    overall_timeout: std::time::Duration,
    response_hook: Option<Arc<dyn crate::transport::ErasedResponseHook>>,
    subscriptions: ValidatedSubscriptionSettings,
    connection_init_authorizer: Arc<dyn chigiri_http::ConnectionInitAuthorizer>,
}

fn transport_settings<C>(parts: &BuildParts<C>) -> TransportSettings {
    TransportSettings {
        body_limit: parts.body_limit,
        response_byte_limit: parts.response_limit,
        body_read_timeout: parts.body_timeout,
        transport_admission_timeout: parts.transport_timeout,
        source_resolution_timeout: parts.source_timeout,
        decoded_request_timeout: parts.decoded_timeout,
        context_timeout: parts.context_timeout,
        execution_timeout: parts.execution_timeout,
        response_hook_timeout: parts.response_hook_timeout,
        request_timeout: parts.overall_timeout,
    }
}

fn prepare<C>(mut draft: Draft<C>) -> Result<BuildParts<C>, EndpointBuildError>
where
    C: Send + Sync + 'static,
{
    let subscriptions = validation::configuration(&mut draft)?;
    let configuration = Arc::new(draft.snapshot());
    let response_hook = draft.response_hook.take();
    let lifecycle = build_lifecycle(&draft);
    let parts = BuildParts {
        body_limit: draft.body_limit.value,
        response_limit: draft.response_limit.value,
        body_timeout: draft.body_timeout.value,
        transport_timeout: draft.transport_timeout.value,
        source_timeout: draft.source_timeout.value,
        decoded_timeout: draft.decoded_timeout.value,
        context_timeout: draft.context_timeout.value,
        execution_timeout: draft.execution_timeout.value,
        response_hook_timeout: draft.response_hook_timeout.value,
        overall_timeout: draft.overall_timeout.value,
        subscriptions,
        connection_init_authorizer: Arc::clone(&draft.connection_init_authorizer.value),
        schema: build_schema(draft)?,
        lifecycle,
        configuration,
        response_hook,
    };
    Ok(parts)
}

fn build_schema<C>(mut draft: Draft<C>) -> Result<ExecutableSchema<C>, EndpointBuildError>
where
    C: Send + Sync + 'static,
{
    let request_ids = draft
        .request_ids
        .configured()
        .ok_or_else(EndpointBuildError::identity)?;
    let introspection = draft.introspection_enabled();
    let apply_generated = |mut builder: chigiri_runtime::ExecutableSchemaBuilder<C>| {
        builder = builder
            .limits(draft.limits.value)
            .document_cache(draft.cache.value)
            .introspection(if introspection {
                IntrospectionPolicy::Enabled
            } else {
                IntrospectionPolicy::Disabled
            })
            .error_id_factory(SharedErrorIds(Arc::clone(&draft.error_ids)))
            .request_id_factory(SharedRequestIds(Arc::clone(request_ids)))
            .field_instrumentation(draft.field_instrumentation);
        if let Some(sampler) = &draft.sampler {
            builder = builder.sampler(SharedSampler(Arc::clone(sampler)));
        }
        if let Some(instrumentation) = &draft.instrumentation {
            builder = builder.instrumentation(Arc::clone(instrumentation));
        }
        if let Some(observer) = &draft.lifecycle_observer {
            builder = builder.lifecycle_observer(SharedLifecycle(Arc::clone(observer)));
        }
        if let Some(observer) = &draft.error_observer {
            builder = builder.error_observer(SharedErrors(Arc::clone(observer)));
        }
        if let Some(plugin) = &draft.stage_b {
            builder = builder.plugin(SharedPlugin(Arc::clone(plugin)));
        }
        for plugin in &draft.plugins {
            builder = builder.plugin(SharedPlugin(Arc::clone(plugin)));
        }
        builder.build().map_err(EndpointBuildError::Schema)
    };
    let EndpointInput { source } = draft
        .input
        .take()
        .expect("The endpoint input is only owned once until the build.");
    match source {
        SchemaSource::Generated(builder) => apply_generated(*builder),
        SchemaSource::Built(schema) => build_existing(&schema, &draft, request_ids),
    }
}

fn build_existing<C>(
    schema: &ExecutableSchema<C>,
    draft: &Draft<C>,
    request_ids: &Arc<dyn chigiri_runtime::RequestIdFactory>,
) -> Result<ExecutableSchema<C>, EndpointBuildError>
where
    C: Send + Sync + 'static,
{
    let mut builder = schema
        .reconfigure()
        .limits(draft.limits.value)
        .document_cache(draft.cache.value)
        .introspection(if draft.introspection_enabled() {
            IntrospectionPolicy::Enabled
        } else {
            IntrospectionPolicy::Disabled
        })
        .error_id_factory(SharedErrorIds(Arc::clone(&draft.error_ids)))
        .request_id_factory(SharedRequestIds(Arc::clone(request_ids)))
        .field_instrumentation(draft.field_instrumentation);
    if let Some(sampler) = &draft.sampler {
        builder = builder.sampler(SharedSampler(Arc::clone(sampler)));
    }
    if let Some(instrumentation) = &draft.instrumentation {
        builder = builder.instrumentation(Arc::clone(instrumentation));
    }
    if let Some(observer) = &draft.lifecycle_observer {
        builder = builder.lifecycle_observer(SharedLifecycle(Arc::clone(observer)));
    }
    if let Some(observer) = &draft.error_observer {
        builder = builder.error_observer(SharedErrors(Arc::clone(observer)));
    }
    if let Some(plugin) = &draft.stage_b {
        builder = builder.prepend_plugin(SharedPlugin(Arc::clone(plugin)));
    }
    for plugin in &draft.plugins {
        builder = builder.plugin(SharedPlugin(Arc::clone(plugin)));
    }
    Ok(builder.build())
}

fn build_lifecycle<C>(draft: &Draft<C>) -> HttpLifecycle {
    let mut lifecycle = HttpLifecycle::new();
    if draft.csrf_enabled {
        lifecycle = lifecycle.transport_admission(CsrfProtection::production());
    }
    for admission in &draft.admissions {
        lifecycle = lifecycle.transport_admission(SharedAdmission(Arc::clone(admission)));
    }
    if let Some(stage) = &draft.stage_a {
        lifecycle = lifecycle.transport_admission(SharedAdmission(Arc::clone(stage)));
    }
    if let Some(source) = &draft.source {
        lifecycle = lifecycle.operation_source(SharedSource(Arc::clone(source)));
    }
    for hook in &draft.decoded {
        lifecycle = lifecycle.decoded_request(SharedDecoded(Arc::clone(hook)));
    }
    lifecycle
}
