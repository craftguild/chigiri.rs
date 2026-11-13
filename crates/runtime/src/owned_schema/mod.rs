//! Experimental owned-SDL input with instance and field-coverage validation.
//!
//! Owned input does not claim generated-ABI provenance.

mod binding;
mod defaults;
mod parse;

use std::collections::BTreeSet;
use std::sync::Arc;

use chigiri_core::executor::{ObjectScope, ResolvedObject};
use chigiri_core::schema::SemanticSchema;
use chigiri_core::{SchemaDigest, Source};

use crate::SchemaBuildError;

pub use binding::{OwnedResolverDispatch, OwnedSchemaInput};
pub use parse::OwnedSchemaError;

/// Experimental input backed by a validated SDL graph.
///
/// Consumers normally use generated resolvers. Mock declaration validation and
/// Scripting integrations use this owned form.
#[derive(Clone)]
pub struct OwnedSchema {
    inner: Arc<OwnedDefinition>,
}

struct OwnedDefinition {
    semantic: Arc<SemanticSchema>,
    sources: Vec<Source>,
    digest: SchemaDigest,
    scope: ObjectScope,
    object_names: BTreeSet<Arc<str>>,
}

impl OwnedSchema {
    /// Returns the type graph verified by parse/compose. The user resolves the name to IR's ID reference.
    pub fn semantic(&self) -> &SemanticSchema {
        &self.inner.semantic
    }

    /// Returns the logical source in the input order that corresponds to the source location and digest.
    pub fn sources(&self) -> &[Source] {
        &self.inner.sources
    }

    /// Returns a source-set digest with the same width as generated schema digests.
    pub fn digest(&self) -> SchemaDigest {
        self.inner.digest
    }

    /// Returns whether this schema is the same instance as another.
    pub fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Binds a known concrete object type name to this schema's ownership scope.
    ///
    /// Abstract and unknown names are rejected. Dispatch validates the value
    /// against the SDL type and stored record, and the type name shares the
    /// schema-owned `Arc`.
    pub fn object<T>(&self, type_name: &str, value: T) -> Result<ResolvedObject, SchemaBuildError>
    where
        T: Send + Sync + 'static,
    {
        let name =
            self.inner.object_names.get(type_name).ok_or_else(|| {
                SchemaBuildError::new("owned object requires a concrete schema type")
            })?;
        Ok(ResolvedObject::owned(
            self.inner.scope.clone(),
            name.clone(),
            value,
        ))
    }
}
