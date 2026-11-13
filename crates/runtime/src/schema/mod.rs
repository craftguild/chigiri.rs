//! Cheaply cloned executable schemas built from descriptors and resolver dispatch.

mod builder;
mod handle;
mod options;
#[cfg(test)]
mod tests;

use std::marker::PhantomData;
use std::sync::Arc;

use chigiri_core::ExecutionLimits;
use chigiri_core::executable::CustomScalarRegistry;
use chigiri_core::executor::{FieldDispatch, Plugin, ResolverDispatch};
use chigiri_core::schema::SemanticSchema;

use crate::diagnostics::Diagnostics;
use crate::subscription::SubscriptionResolverDispatch;
use crate::{SchemaDescriptor, SchemaInfo, cache};

pub use options::{DocumentCacheLimits, IntrospectionPolicy, SchemaBuildError};

/// `Arc`-backed executable schema handle shared between requests.
pub struct ExecutableSchema<C> {
    pub(crate) inner: Arc<SchemaInner<C>>,
}

impl<C> Clone for ExecutableSchema<C> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

pub(crate) struct SchemaInner<C> {
    pub(crate) info: SchemaInfo,
    pub(crate) semantic_schema: Arc<SemanticSchema>,
    pub(crate) resolvers: Arc<dyn FieldDispatch<C>>,
    pub(crate) subscription: Option<Arc<dyn SubscriptionResolverDispatch<C>>>,
    pub(crate) scalars: Arc<CustomScalarRegistry>,
    pub(crate) limits: ExecutionLimits,
    pub(crate) introspection: IntrospectionPolicy,
    pub(crate) cache: cache::DocumentCache,
    pub(crate) plugins: Vec<Arc<dyn Plugin<C>>>,
    pub(crate) diagnostics: Diagnostics,
}

/// Builder for an executable schema with explicit policies.
pub struct ExecutableSchemaBuilder<C> {
    descriptor: &'static SchemaDescriptor,
    resolvers: Arc<dyn ResolverDispatch<C>>,
    subscription: Option<Arc<dyn SubscriptionResolverDispatch<C>>>,
    limits: ExecutionLimits,
    introspection: IntrospectionPolicy,
    cache_limits: DocumentCacheLimits,
    plugins: Vec<Arc<dyn Plugin<C>>>,
    diagnostics: Diagnostics,
    catalog_revision: Option<Arc<str>>,
    expected_resolver_abi: Option<chigiri_core::executor::ResolverSchemaFingerprint>,
    marker: PhantomData<fn(C)>,
}
