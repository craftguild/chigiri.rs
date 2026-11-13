//! Owned schema and dispatch binding by instance, exact field set, and scalar registry.

use std::collections::BTreeSet;
use std::sync::Arc;

use chigiri_core::FieldResult;
use chigiri_core::executable::CustomScalarRegistry;
use chigiri_core::executor::{
    ArgumentValues, BoxFuture, FieldDispatch, ObjectScope, ResolvedObject, ResolvedValue,
};
use chigiri_core::schema::TypeKind;

use crate::schema::SchemaInner;
use crate::subscription::SubscriptionResolverDispatch;
use crate::{
    DocumentCacheLimits, ExecutableSchema, ExecutionLimits, IntrospectionPolicy, SchemaBuildError,
    SchemaInfo, cache,
};

use super::OwnedSchema;

/// Dispatch for an experimental owned schema, separate from the complete generated-trait contract.
pub trait OwnedResolverDispatch<C>: Send + Sync + 'static {
    /// Returns the exact schema instance bound to this dispatch.
    fn schema(&self) -> &OwnedSchema;

    /// Returns the exact set of all concrete object fields.
    ///
    /// `bind` rejects duplicate, missing, and unknown coordinates.
    fn coordinates(&self) -> &[(Arc<str>, Arc<str>)];

    /// Registers every custom scalar into the supplied registry.
    fn register_custom_scalars(&self, registry: &mut CustomScalarRegistry);

    /// Calls a type-checked field plan. Implementations must still validate
    /// business constraints on returned values.
    fn resolve<'a>(
        &'a self,
        context: &'a C,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>>;
}

/// Validated owned-schema input passed to `Endpoint::builder` before final policies apply.
pub struct OwnedSchemaInput<C>(ExecutableSchema<C>);

impl<C> OwnedSchemaInput<C> {
    /// The facade applies its component graph by reconfiguring the internal schema.
    #[doc(hidden)]
    pub fn into_executable(self) -> ExecutableSchema<C> {
        self.0
    }
}

impl OwnedSchema {
    /// Validates schema identity, dispatch and field coverage, and scalar
    /// registration, then creates Endpoint input from these bindings.
    pub fn bind<C, D>(self, dispatch: D) -> Result<OwnedSchemaInput<C>, SchemaBuildError>
    where
        C: Send + Sync + 'static,
        D: OwnedResolverDispatch<C>,
    {
        self.bind_dispatch(Arc::new(dispatch), None)
    }

    /// Binds ordinary and subscription dispatch from the same owned assembly.
    ///
    /// The subscription root must exactly match the owned schema. Ordinary
    /// field coverage, scalar registration, and instance checks remain required.
    #[doc(hidden)]
    pub fn bind_with_subscriptions<C, D>(
        self,
        dispatch: D,
    ) -> Result<OwnedSchemaInput<C>, SchemaBuildError>
    where
        C: Send + Sync + 'static,
        D: OwnedResolverDispatch<C> + SubscriptionResolverDispatch<C>,
    {
        if self
            .semantic()
            .roots()
            .subscription()
            .map(|root| root.type_name.as_str())
            != dispatch.subscription_root()
        {
            return Err(SchemaBuildError::new(
                "owned schema and subscription dispatch root do not match",
            ));
        }
        let dispatch = Arc::new(dispatch);
        let subscription: Arc<dyn SubscriptionResolverDispatch<C>> = dispatch.clone();
        self.bind_dispatch(dispatch, Some(subscription))
    }

    fn bind_dispatch<C, D>(
        self,
        dispatch: Arc<D>,
        subscription: Option<Arc<dyn SubscriptionResolverDispatch<C>>>,
    ) -> Result<OwnedSchemaInput<C>, SchemaBuildError>
    where
        C: Send + Sync + 'static,
        D: OwnedResolverDispatch<C>,
    {
        if !self.same_instance(dispatch.schema()) {
            return Err(SchemaBuildError::new(
                "owned schema and dispatch instances differ",
            ));
        }
        let expected: BTreeSet<_> = self
            .semantic()
            .types()
            .iter()
            .filter(|ty| ty.kind() == TypeKind::Object)
            .flat_map(|ty| {
                ty.fields()
                    .iter()
                    .map(move |f| (ty.name(), f.name.value.as_str()))
            })
            .collect();
        let coordinates = dispatch.coordinates();
        if coordinates.len() != expected.len()
            || coordinates
                .iter()
                .map(|(t, f)| (t.as_ref(), f.as_ref()))
                .collect::<BTreeSet<_>>()
                != expected
        {
            return Err(SchemaBuildError::new(
                "owned resolver coordinates must exactly cover object fields",
            ));
        }
        let mut scalars = CustomScalarRegistry::new();
        dispatch.register_custom_scalars(&mut scalars);
        let scalar_names: BTreeSet<_> = self
            .semantic()
            .types()
            .iter()
            .filter(|ty| ty.kind() == TypeKind::Scalar && ty.name_span().is_some())
            .filter(|ty| !matches!(ty.name(), "Int" | "Float" | "String" | "Boolean" | "ID"))
            .map(|ty| ty.name())
            .collect();
        if scalars.names().collect::<BTreeSet<_>>() != scalar_names {
            return Err(SchemaBuildError::new(
                "owned scalar registrations must exactly cover custom scalars",
            ));
        }
        super::defaults::validate(self.semantic(), &scalars)?;
        let schema = ExecutableSchema {
            inner: Arc::new(SchemaInner {
                info: SchemaInfo::owned(env!("CARGO_PKG_VERSION"), self.digest()),
                subscription,
                semantic_schema: Arc::clone(&self.inner.semantic),
                resolvers: Arc::new(OwnedDispatch {
                    schema: self,
                    dispatch,
                }),
                scalars: Arc::new(scalars),
                limits: ExecutionLimits::production(),
                introspection: IntrospectionPolicy::Disabled,
                cache: cache::DocumentCache::new(DocumentCacheLimits::default()),
                plugins: Vec::new(),
                diagnostics: crate::diagnostics::Diagnostics::default(),
            }),
        };
        Ok(OwnedSchemaInput(schema))
    }
}

struct OwnedDispatch<D> {
    schema: OwnedSchema,
    dispatch: Arc<D>,
}

impl<C, D: OwnedResolverDispatch<C>> FieldDispatch<C> for OwnedDispatch<D> {
    fn object_scope(&self) -> Option<&ObjectScope> {
        Some(&self.schema.inner.scope)
    }

    fn runtime_byte_limit(&self) -> Option<usize> {
        Some(super::defaults::COERCION_BYTES)
    }

    fn input_coercion_limits(&self) -> Option<(usize, usize)> {
        Some((
            super::defaults::COERCION_NODES,
            super::defaults::COERCION_BYTES,
        ))
    }

    fn resolve<'a>(
        &'a self,
        context: &'a C,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.dispatch
            .resolve(context, parent_type, field_name, parent, arguments)
    }
}
