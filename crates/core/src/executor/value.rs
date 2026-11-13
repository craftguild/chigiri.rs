//! Values and futures exchanged between application resolvers and the core executor.

use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::field_dispatch::ObjectScope;

use crate::executable::{CoercedInput, CustomScalarRegistry};
use crate::{FieldResult, Value};

use super::fingerprint::ResolverSchemaFingerprint;

/// Type-erased representation of the future returned by a generated resolver.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Coerced arguments arranged in schema-field definition order.
pub struct ArgumentValues {
    values: Vec<CoercedInput>,
}

/// Result of attempting a generated dispatch while preserving unconsumed arguments.
pub enum DispatchAttempt<T> {
    /// Forwards the argument values to the next dispatch chunk.
    Unmatched(ArgumentValues),
    /// Resolver future that matches schema coordinates.
    Matched(T),
}

impl ArgumentValues {
    /// Retains arguments in field-definition order.
    pub fn new(values: Vec<CoercedInput>) -> Self {
        Self { values }
    }

    /// Returns the value to a generated converter.
    pub fn into_values(self) -> std::vec::IntoIter<CoercedInput> {
        self.values.into_iter()
    }

    pub(super) fn iter(&self) -> impl ExactSizeIterator<Item = &CoercedInput> {
        self.values.iter()
    }
}

/// Runtime object value and GraphQL type name passed to child resolvers.
pub struct ResolvedObject {
    type_name: ObjectTypeName,
    scope: Option<ObjectScope>,
    value: Box<dyn Any + Send + Sync>,
}

impl ResolvedObject {
    /// Type-erases a concrete application object.
    pub fn new<T>(type_name: &'static str, value: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self {
            type_name: ObjectTypeName::Static(type_name),
            scope: None,
            value: Box::new(value),
        }
    }

    /// Returns the GraphQL runtime type name.
    pub fn type_name(&self) -> &str {
        match &self.type_name {
            ObjectTypeName::Static(name) => name,
            ObjectTypeName::Owned(name) => name,
        }
    }

    /// Binds an object to an owned schema so completion can check runtime type
    /// and scope compatibility.
    pub fn owned<T>(scope: ObjectScope, type_name: Arc<str>, value: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self {
            type_name: ObjectTypeName::Owned(type_name),
            scope: Some(scope),
            value: Box::new(value),
        }
    }

    pub(super) fn belongs_to(&self, expected: Option<&ObjectScope>) -> bool {
        match (&self.scope, expected) {
            (None, None) => true,
            (Some(actual), Some(expected)) => actual.same_instance(expected),
            _ => false,
        }
    }

    /// Generated adapters downcast this value to the expected application object.
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.value.downcast_ref()
    }
}

enum ObjectTypeName {
    Static(&'static str),
    Owned(Arc<str>),
}

/// Type-erased value before resolver-output completion.
pub enum ResolvedValue {
    /// GraphQL null.
    Null,
    /// The scalar or enum leaf value.
    Leaf(Value),
    /// List-item values.
    List(Vec<FieldResult<ResolvedValue>>),
    /// Concrete application object.
    Object(ResolvedObject),
}

impl ResolvedValue {
    /// Type-erases an application object together with its runtime type name.
    pub fn object<T>(type_name: &'static str, value: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self::Object(ResolvedObject::new(type_name, value))
    }
}

/// The only dynamic-dispatch boundary that generated `Resolvers` exposes to
/// the executor.
pub trait ResolverDispatch<C>: Send + Sync {
    /// Returns the typed-adapter ABI fingerprint from the generated resolver table.
    fn schema_fingerprint(&self) -> ResolverSchemaFingerprint;

    /// Returns the generated Rust ABI fingerprint that includes configuration.
    fn resolver_abi_fingerprint(&self) -> ResolverSchemaFingerprint {
        self.schema_fingerprint()
    }

    /// Registers the application custom scalar's input validator.
    fn register_custom_scalars(&self, registry: &mut CustomScalarRegistry);

    /// Calls a schema-coordinate resolver and type-erases its output.
    fn resolve<'a>(
        &'a self,
        context: &'a C,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>>;
}
