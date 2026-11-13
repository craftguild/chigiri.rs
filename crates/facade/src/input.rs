//! Type-erases generated resolvers and already-built schemas into a common endpoint input.

use chigiri_runtime::__private::{
    ResolverDispatch, ResolverSchemaFingerprint, SubscriptionResolverDispatch,
};
use chigiri_runtime::{ExecutableSchema, ExecutableSchemaBuilder, SchemaDescriptor};

/// Closed schema-input contract used when building an endpoint.
#[doc(hidden)]
pub struct EndpointInput<C> {
    pub(crate) source: SchemaSource<C>,
}

pub(crate) enum SchemaSource<C> {
    Generated(Box<ExecutableSchemaBuilder<C>>),
    Built(ExecutableSchema<C>),
}

impl<C> EndpointInput<C>
where
    C: Send + Sync + 'static,
{
    /// Passes ABI-compatible generated resolver dispatch to the endpoint builder.
    #[doc(hidden)]
    pub fn generated<D>(
        descriptor: &'static SchemaDescriptor,
        fingerprint: ResolverSchemaFingerprint,
        dispatch: D,
    ) -> Self
    where
        D: ResolverDispatch<C> + SubscriptionResolverDispatch<C>,
    {
        Self {
            source: SchemaSource::Generated(Box::new(
                ExecutableSchema::builder_with_subscriptions(descriptor, dispatch)
                    .resolver_abi_fingerprint(fingerprint),
            )),
        }
    }
}

/// Closed schema-entry contracts accepted by `Endpoint::builder`.
///
/// Built-in assembly passes identity, resolution, scalar, and catalog components explicitly;
/// execution components are not inherited.
pub trait IntoEndpointInput<C> {
    #[doc(hidden)]
    fn into_endpoint_input(self) -> EndpointInput<C>;
}

impl<C> IntoEndpointInput<C> for ExecutableSchema<C> {
    fn into_endpoint_input(self) -> EndpointInput<C> {
        EndpointInput {
            source: SchemaSource::Built(self),
        }
    }
}

impl<C> IntoEndpointInput<C> for chigiri_runtime::OwnedSchemaInput<C> {
    fn into_endpoint_input(self) -> EndpointInput<C> {
        EndpointInput {
            source: SchemaSource::Built(self.into_executable()),
        }
    }
}
