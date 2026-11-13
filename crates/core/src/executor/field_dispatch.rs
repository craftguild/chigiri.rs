//! Transfers verified input to the executor while separating generated-ABI checks from owned-instance checks.

use std::sync::Arc;

use crate::FieldResult;

use super::{ArgumentValues, BoxFuture, ResolvedObject, ResolvedValue};

/// Object-identity scope for an owned schema. Only clones share the same instance.
#[derive(Clone, Default)]
pub struct ObjectScope(Arc<()>);

impl ObjectScope {
    /// Create a new scope distinct from every existing scope.
    pub fn new() -> Self {
        Self::default()
    }

    /// Compares erased values by concrete type identity, not contents or names.
    pub fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Post-construction-validated field-call contract implemented by generated
/// adapters; the builder maintains generated-trait completeness.
pub trait FieldDispatch<C>: Send + Sync {
    /// Return the owned-object scope, or `None` for generated inputs.
    fn object_scope(&self) -> Option<&ObjectScope>;

    /// Returns coercion limits for owned requests as node and logical-byte counts.
    fn input_coercion_limits(&self) -> Option<(usize, usize)> {
        None
    }

    /// The logical byte limit for each of the owned request metadata and response holds.
    fn runtime_byte_limit(&self) -> Option<usize> {
        None
    }

    /// Executes the field with a parent object from the same instance as the
    /// coerced arguments.
    fn resolve<'a>(
        &'a self,
        context: &'a C,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>>;
}
