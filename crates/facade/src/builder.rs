//! Context typestate and endpoint builders backed by a shared component graph.

use std::marker::PhantomData;
use std::time::Duration;

use chigiri_http::{
    ConnectionInitAuthorizer, ContextFactory, OperationSourceResolver, ResponseHook,
};
use chigiri_runtime::{DocumentCacheLimits, ExecutionLimits, Plugin};

use crate::build_support::UnitContextFactory;
use crate::component_ops;
use crate::components::Components;
use crate::endpoint::{ContextPending, EndpointBuilder, MissingContextEndpointBuilder};
use crate::rate::RateStage;
use crate::subscription::settings::{SubscriptionLimits, SubscriptionProtocolSettings};

impl<C> MissingContextEndpointBuilder<C>
where
    C: Send + Sync + 'static,
{
    /// The default is false.
    ///
    /// Setting this back to `false` disables only GraphiQL publication. Existing
    /// policies such as CSRF and rate limiting remain configured.
    #[cfg(feature = "graphiql")]
    pub fn graphiql(mut self, enabled: bool) -> Self {
        crate::draft::Draft::<C>::set(&mut self.draft.sequence, &mut self.draft.graphiql, enabled);
        self
    }

    /// Sets a per-request context factory and returns a buildable endpoint.
    pub fn context_factory<S, F>(self, factory: F) -> EndpointBuilder<C, S, F>
    where
        S: Send + Sync + 'static,
        F: ContextFactory<C, S>,
    {
        EndpointBuilder {
            draft: self.draft,
            factory,
            marker: PhantomData,
        }
    }

    /// Edits the component graph through a synchronous closure.
    pub fn inject(mut self, edit: impl FnOnce(&mut Components<'_, C, ContextPending>)) -> Self {
        edit(&mut Components {
            draft: &mut self.draft,
            response_state: PhantomData::<fn() -> ContextPending>,
        });
        self
    }

    /// Set the rate limit for Stage A or B to the corresponding slot.
    pub fn rate_limit<R: RateStage<C>>(mut self, stage: R) -> Self {
        crate::rate::apply_stage(stage, &mut self.draft);
        self
    }

    /// Sets structural limits from parsing through response completion.
    pub fn execution_limits(mut self, limits: ExecutionLimits) -> Self {
        component_ops::execution_limits(&mut self.draft, limits);
        self
    }
    /// Sets document-cache entry and byte limits.
    pub fn document_cache(mut self, limits: DocumentCacheLimits) -> Self {
        component_ops::document_cache(&mut self.draft, limits);
        self
    }
    /// Sets the request-body byte limit, which must be less than 1 GiB.
    pub fn body_limit(mut self, bytes: usize) -> Self {
        component_ops::body_limit(&mut self.draft, bytes);
        self
    }
    /// Sets the encoded-response byte limit, which must be less than 1 GiB.
    pub fn response_byte_limit(mut self, bytes: usize) -> Self {
        component_ops::response_byte_limit(&mut self.draft, bytes);
        self
    }
    /// Sets a GraphQL execution deadline shorter than 24 hours.
    pub fn execution_timeout(mut self, timeout: Duration) -> Self {
        component_ops::execution_timeout(&mut self.draft, timeout);
        self
    }
    /// Replaces the complete endpoint and per-subscription delivery limits.
    pub fn subscription_limits(mut self, limits: SubscriptionLimits) -> Self {
        component_ops::subscription_limits(&mut self.draft, limits);
        self
    }
    /// Replaces the complete WebSocket subscription protocol setting.
    pub fn subscription_protocol(mut self, settings: SubscriptionProtocolSettings) -> Self {
        component_ops::subscription_protocol(&mut self.draft, settings);
        self
    }
    /// Replaces the WebSocket connection-initialization authorizer.
    pub fn connection_init_authorizer(mut self, authorizer: impl ConnectionInitAuthorizer) -> Self {
        component_ops::connection_init_authorizer(&mut self.draft, authorizer);
        self
    }
    /// Set the raw or persisted operation solver.
    pub fn operation_source(mut self, source: impl OperationSourceResolver) -> Self {
        component_ops::operation_source(&mut self.draft, source);
        self
    }
    /// Add a plugin to the end of an existing collection.
    pub fn push_plugin(mut self, plugin: impl Plugin<C>) -> Self {
        component_ops::push_plugin(&mut self.draft, plugin);
        self
    }
}

impl MissingContextEndpointBuilder<()> {
    /// Select and build the unit context.
    pub fn without_context(self) -> EndpointBuilder<(), (), impl ContextFactory<(), ()>> {
        EndpointBuilder {
            draft: self.draft,
            factory: UnitContextFactory,
            marker: PhantomData,
        }
    }
}

impl<C, S, F> EndpointBuilder<C, S, F>
where
    C: Send + Sync + 'static,
    S: Send + Sync + 'static,
    F: ContextFactory<C, S>,
{
    /// The default is false.
    ///
    /// Setting this back to `false` disables only GraphiQL publication. Existing
    /// policies such as CSRF and rate limiting remain configured.
    #[cfg(feature = "graphiql")]
    pub fn graphiql(mut self, enabled: bool) -> Self {
        crate::draft::Draft::<C>::set(&mut self.draft.sequence, &mut self.draft.graphiql, enabled);
        self
    }

    /// Sets the response-header hook that runs after execution.
    pub fn response_hook<H>(mut self, hook: H) -> Self
    where
        H: ResponseHook<S>,
    {
        component_ops::response_hook(&mut self.draft, hook);
        self
    }

    /// Edits the component graph through a synchronous closure.
    pub fn inject(mut self, edit: impl FnOnce(&mut Components<'_, C, S>)) -> Self {
        edit(&mut Components {
            draft: &mut self.draft,
            response_state: PhantomData::<fn() -> S>,
        });
        self
    }

    /// Set the rate limit for Stage A or B to the corresponding slot.
    pub fn rate_limit<R: RateStage<C>>(mut self, stage: R) -> Self {
        crate::rate::apply_stage(stage, &mut self.draft);
        self
    }

    /// Sets structural limits from parsing through response completion.
    pub fn execution_limits(mut self, limits: ExecutionLimits) -> Self {
        component_ops::execution_limits(&mut self.draft, limits);
        self
    }
    /// Sets document-cache entry and byte limits.
    pub fn document_cache(mut self, limits: DocumentCacheLimits) -> Self {
        component_ops::document_cache(&mut self.draft, limits);
        self
    }
    /// Sets the request-body byte limit, which must be less than 1 GiB.
    pub fn body_limit(mut self, bytes: usize) -> Self {
        component_ops::body_limit(&mut self.draft, bytes);
        self
    }
    /// Sets the encoded-response byte limit, which must be less than 1 GiB.
    pub fn response_byte_limit(mut self, bytes: usize) -> Self {
        component_ops::response_byte_limit(&mut self.draft, bytes);
        self
    }
    /// Sets a GraphQL execution deadline shorter than 24 hours.
    pub fn execution_timeout(mut self, timeout: Duration) -> Self {
        component_ops::execution_timeout(&mut self.draft, timeout);
        self
    }
    /// Replaces the complete endpoint and per-subscription delivery limits.
    pub fn subscription_limits(mut self, limits: SubscriptionLimits) -> Self {
        component_ops::subscription_limits(&mut self.draft, limits);
        self
    }
    /// Replaces the complete WebSocket subscription protocol setting.
    pub fn subscription_protocol(mut self, settings: SubscriptionProtocolSettings) -> Self {
        component_ops::subscription_protocol(&mut self.draft, settings);
        self
    }
    /// Replaces the WebSocket connection-initialization authorizer.
    pub fn connection_init_authorizer(mut self, authorizer: impl ConnectionInitAuthorizer) -> Self {
        component_ops::connection_init_authorizer(&mut self.draft, authorizer);
        self
    }
    /// Set the raw or persisted operation solver.
    pub fn operation_source(mut self, source: impl OperationSourceResolver) -> Self {
        component_ops::operation_source(&mut self.draft, source);
        self
    }
    /// Add a plugin to the end of an existing collection.
    pub fn push_plugin(mut self, plugin: impl Plugin<C>) -> Self {
        component_ops::push_plugin(&mut self.draft, plugin);
        self
    }
}
