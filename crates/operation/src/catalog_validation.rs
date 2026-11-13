//! Offline schema validation for operation catalogs.

use std::fmt;
use std::sync::Arc;

use chigiri_runtime::{ExecutableSchema, GraphQlRequest, OperationIdentity, ValidatedOperation};

use crate::{CatalogRevision, OperationCatalog, PersistedOperationId};

/// Schema-validated catalog-entry metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedCatalogEntry {
    id: PersistedOperationId,
    operation: ValidatedOperation,
    cost_hint: Option<u64>,
}

impl ValidatedCatalogEntry {
    pub const fn id(&self) -> &PersistedOperationId {
        &self.id
    }
    pub const fn identity(&self) -> &OperationIdentity {
        self.operation.identity()
    }
    pub const fn operation(&self) -> &ValidatedOperation {
        &self.operation
    }
    pub const fn cost_hint(&self) -> Option<u64> {
        self.cost_hint
    }
}

/// An immutable catalog snapshot in which every entry is valid against the
/// same schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedCatalog {
    revision: CatalogRevision,
    entries: Arc<[ValidatedCatalogEntry]>,
}

impl ValidatedCatalog {
    pub const fn revision(&self) -> &CatalogRevision {
        &self.revision
    }
    pub fn entries(&self) -> &[ValidatedCatalogEntry] {
        &self.entries
    }
}

/// The validation error returns the schema incompatibility entry with the exact ID set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogValidationError {
    invalid_ids: Vec<PersistedOperationId>,
}

impl CatalogValidationError {
    pub fn invalid_ids(&self) -> &[PersistedOperationId] {
        &self.invalid_ids
    }
}

impl fmt::Display for CatalogValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "operation catalog has {} schema-incompatible entries",
            self.invalid_ids.len()
        )
    }
}

impl std::error::Error for CatalogValidationError {}

/// Parses and validates every catalog entry, including operation selection.
pub fn validate_catalog<C>(
    schema: &ExecutableSchema<C>,
    catalog: &dyn OperationCatalog,
) -> Result<ValidatedCatalog, CatalogValidationError>
where
    C: Send + Sync + 'static,
{
    let mut validated = Vec::with_capacity(catalog.entries().len());
    let mut invalid_ids = Vec::new();
    for entry in catalog.entries() {
        let mut request = GraphQlRequest::new(entry.document().as_ref());
        request.operation_name = entry
            .operation_name()
            .map(|operation_name| operation_name.as_str().to_owned());
        match schema.validate_operation_metadata(&request) {
            Ok(operation) => validated.push(ValidatedCatalogEntry {
                id: entry.id().clone(),
                operation,
                cost_hint: entry.cost_hint(),
            }),
            Err(_) => invalid_ids.push(entry.id().clone()),
        }
    }
    if !invalid_ids.is_empty() {
        return Err(CatalogValidationError { invalid_ids });
    }
    Ok(ValidatedCatalog {
        revision: catalog.revision().clone(),
        entries: validated.into(),
    })
}
