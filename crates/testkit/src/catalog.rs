//! Closed model for schema and cost impact on application-owned operation catalogs.

use std::collections::BTreeMap;
use std::fmt;

use chigiri::ValidatedOperation;
use chigiri::spi::ExecutableSchema;
use chigiri_operation::{
    CatalogRevision, CatalogValidationError, OperationCatalog, PersistedOperationId,
    validate_catalog,
};
use chigiri_policy::{OperationCostContext, OperationCostPolicy};
use chigiri_rate::OperationCost;

/// Validation, result-shape, and cost snapshot for one exact persisted ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogImpactEntry {
    id: PersistedOperationId,
    operation: ValidatedOperation,
    cost: OperationCost,
}

impl CatalogImpactEntry {
    pub const fn id(&self) -> &PersistedOperationId {
        &self.id
    }
    pub const fn operation(&self) -> &ValidatedOperation {
        &self.operation
    }
    pub const fn cost(&self) -> OperationCost {
        self.cost
    }
}

/// Catalog-impact snapshot evaluated against a schema and cost policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogImpactSnapshot {
    revision: CatalogRevision,
    entries: Vec<CatalogImpactEntry>,
}

impl CatalogImpactSnapshot {
    pub const fn revision(&self) -> &CatalogRevision {
        &self.revision
    }
    pub fn entries(&self) -> &[CatalogImpactEntry] {
        &self.entries
    }
}

/// Change detected for one exact ID between baseline and current snapshots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogImpactChangeKind {
    Added,
    Removed,
    Changed,
}

/// Rollout decision for identity, result-shape, and structural-cost changes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogImpactChange {
    id: PersistedOperationId,
    kind: CatalogImpactChangeKind,
    identity_changed: bool,
    result_shape_changed: bool,
    complexity_changed: bool,
    baseline_cost: Option<OperationCost>,
    current_cost: Option<OperationCost>,
}

impl CatalogImpactChange {
    pub const fn id(&self) -> &PersistedOperationId {
        &self.id
    }
    pub const fn kind(&self) -> CatalogImpactChangeKind {
        self.kind
    }
    pub const fn identity_changed(&self) -> bool {
        self.identity_changed
    }
    pub const fn result_shape_changed(&self) -> bool {
        self.result_shape_changed
    }
    pub const fn complexity_changed(&self) -> bool {
        self.complexity_changed
    }
    pub const fn baseline_cost(&self) -> Option<OperationCost> {
        self.baseline_cost
    }
    pub const fn current_cost(&self) -> Option<OperationCost> {
        self.current_cost
    }
}

/// Returns schema validation or cost policy failure with exact ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogImpactError {
    Validation(CatalogValidationError),
    Cost(Vec<PersistedOperationId>),
}

impl CatalogImpactError {
    pub fn invalid_ids(&self) -> &[PersistedOperationId] {
        match self {
            Self::Validation(error) => error.invalid_ids(),
            Self::Cost(ids) => ids,
        }
    }
}

impl fmt::Display for CatalogImpactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "operation catalog impact failed for {} entries",
            self.invalid_ids().len()
        )
    }
}

impl std::error::Error for CatalogImpactError {}

/// Validates every catalog entry against a schema and snapshots result shape and cost.
pub fn validate_catalog_impact<C>(
    schema: &ExecutableSchema<C>,
    catalog: &dyn OperationCatalog,
    cost_policy: &dyn OperationCostPolicy,
) -> Result<CatalogImpactSnapshot, CatalogImpactError>
where
    C: Send + Sync + 'static,
{
    let validated = validate_catalog(schema, catalog).map_err(CatalogImpactError::Validation)?;
    let mut entries = Vec::with_capacity(validated.entries().len());
    let mut failed = Vec::new();
    for entry in validated.entries() {
        let operation = entry.operation();
        let complexity = operation.complexity();
        let catalog_override = entry
            .cost_hint()
            .and_then(|value| OperationCost::new(value).ok());
        let context = OperationCostContext::new(
            operation.identity(),
            complexity.expanded_fields as u64,
            complexity.aliases as u64,
            complexity.depth as u64,
            catalog_override,
            operation.cost_fields(),
        );
        match cost_policy.cost(context) {
            Ok(cost) => entries.push(CatalogImpactEntry {
                id: entry.id().clone(),
                operation: operation.clone(),
                cost,
            }),
            Err(_) => failed.push(entry.id().clone()),
        }
    }
    if !failed.is_empty() {
        return Err(CatalogImpactError::Cost(failed));
    }
    Ok(CatalogImpactSnapshot {
        revision: validated.revision().clone(),
        entries,
    })
}

/// Compares all exact IDs in baseline and current snapshots and returns changes.
pub fn diff_catalog_impact(
    baseline: &CatalogImpactSnapshot,
    current: &CatalogImpactSnapshot,
) -> Vec<CatalogImpactChange> {
    let old = baseline
        .entries
        .iter()
        .map(|entry| (entry.id(), entry))
        .collect::<BTreeMap<_, _>>();
    let new = current
        .entries
        .iter()
        .map(|entry| (entry.id(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();
    for (id, entry) in &old {
        match new.get(id) {
            None => changes.push(removed(entry)),
            Some(current) if *entry != *current => changes.push(changed(entry, current)),
            Some(_) => {}
        }
    }
    for (id, entry) in &new {
        if !old.contains_key(id) {
            changes.push(added(entry));
        }
    }
    changes
}

fn changed(old: &CatalogImpactEntry, new: &CatalogImpactEntry) -> CatalogImpactChange {
    CatalogImpactChange {
        id: old.id.clone(),
        kind: CatalogImpactChangeKind::Changed,
        identity_changed: old.operation.identity() != new.operation.identity(),
        result_shape_changed: old.operation.result_shape_digest()
            != new.operation.result_shape_digest(),
        complexity_changed: old.operation.complexity() != new.operation.complexity(),
        baseline_cost: Some(old.cost),
        current_cost: Some(new.cost),
    }
}

fn added(entry: &CatalogImpactEntry) -> CatalogImpactChange {
    edge(
        entry,
        CatalogImpactChangeKind::Added,
        None,
        Some(entry.cost),
    )
}

fn removed(entry: &CatalogImpactEntry) -> CatalogImpactChange {
    edge(
        entry,
        CatalogImpactChangeKind::Removed,
        Some(entry.cost),
        None,
    )
}

fn edge(
    entry: &CatalogImpactEntry,
    kind: CatalogImpactChangeKind,
    baseline_cost: Option<OperationCost>,
    current_cost: Option<OperationCost>,
) -> CatalogImpactChange {
    CatalogImpactChange {
        id: entry.id.clone(),
        kind,
        identity_changed: true,
        result_shape_changed: true,
        complexity_changed: true,
        baseline_cost,
        current_cost,
    }
}
