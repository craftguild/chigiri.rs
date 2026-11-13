//! Own model state, candidates, and snapshots, swapping in only fully validated candidates.

mod candidate;
mod edit;
mod identity;
mod implicit;
mod index_usage;
mod key_reference;
mod keys;
mod lookup;
mod patch;
mod publication;
mod query;
mod query_plan;
mod reader;
mod records;
mod relation_edit;
mod relations;
mod request;
mod seed;
mod seed_capacity;
mod seed_data;
mod seed_json;
mod seed_ref_dependencies;
mod seed_ref_plan;
mod seed_refs;
mod seed_relations;
mod seed_render;
mod selected_write;
#[cfg(test)]
pub(crate) mod test_selection;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_automatic;
#[cfg(test)]
mod tests_candidate_budget;
#[cfg(test)]
mod tests_constraint_scope;
#[cfg(test)]
mod tests_keys;
#[cfg(test)]
mod tests_limits;
#[cfg(test)]
mod tests_patch_keys;
#[cfg(test)]
mod tests_query;
#[cfg(test)]
mod tests_query_cancellation;
#[cfg(test)]
mod tests_query_limits;
#[cfg(test)]
mod tests_relation_edit;
#[cfg(test)]
mod tests_relation_guards;
#[cfg(test)]
mod tests_relation_order;
#[cfg(test)]
mod tests_relations;
#[cfg(test)]
mod tests_seed_data;
#[cfg(test)]
mod tests_seed_json;
#[cfg(test)]
mod tests_seed_refs;
#[cfg(test)]
mod tests_seed_relations;
#[cfg(test)]
mod tests_selected_write;
#[cfg(test)]
mod tests_selection;
#[cfg(test)]
mod tests_work_boundaries;
mod traversal;
mod work;

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::ir::{MockIr, ModelId};
use crate::value::{MAX_BYTES, MAX_NODES, ValueBudget, ValueError};

pub use reader::Reader;
pub use records::Record;
pub use request::{RequestCandidate, RequestPrepared};
pub use seed_data::{SeedData, SeedError};
pub use traversal::Related;
use work::Work;

/// Maximum number of records for each model.
pub const MODEL_RECORDS: usize = 1000;
/// Maximum total number of records across all models.
pub const TOTAL_RECORDS: usize = 2000;

/// Store failure that does not expose a value or ID in its diagnostic.
pub type StoreError = ValueError;

/// Call location authorized by the resolver dispatcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallScope {
    /// Query root is read-only.
    Query,
    /// Mutation root, which may write.
    MutationRoot,
    /// Read-only snapshot exposed to nested fields under a mutation root.
    Nested,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Usage {
    records: usize,
    nodes: usize,
    bytes: usize,
}

impl Usage {
    fn replacing(self, old: Self, new: Self) -> Result<Self, StoreError> {
        let result = Self {
            records: self
                .records
                .checked_sub(old.records)
                .and_then(|n| n.checked_add(new.records))
                .ok_or(ValueError("store capacity"))?,
            nodes: self
                .nodes
                .checked_sub(old.nodes)
                .and_then(|n| n.checked_add(new.nodes))
                .ok_or(ValueError("store capacity"))?,
            bytes: self
                .bytes
                .checked_sub(old.bytes)
                .and_then(|n| n.checked_add(new.bytes))
                .ok_or(ValueError("store capacity"))?,
        };
        if result.records > TOTAL_RECORDS || result.nodes > MAX_NODES || result.bytes > MAX_BYTES {
            return Err(ValueError("store capacity"));
        }
        Ok(result)
    }
}

#[derive(Clone, Debug)]
struct ModelState {
    records: BTreeMap<u64, Arc<Record>>,
    ids: BTreeMap<Arc<str>, u64>,
    next_order: u64,
}

impl ModelState {
    fn new() -> Self {
        Self {
            records: BTreeMap::new(),
            ids: BTreeMap::new(),
            next_order: 0,
        }
    }
}

type RelationIndex = BTreeMap<keys::KeyValue, BTreeMap<u64, u64>>;
type KeyIndex = BTreeMap<keys::KeyValue, u64>;

#[derive(Clone, Debug)]
struct State {
    models: Vec<ModelState>,
    relations: Vec<Arc<RelationIndex>>,
    next_edge: u64,
    keys: Vec<Arc<KeyIndex>>,
    usage: Usage,
}

/// Immutable snapshot of all model state, bound to returned records.
#[derive(Clone)]
pub struct Snapshot {
    ir: Arc<MockIr>,
    state: Arc<State>,
}

impl Snapshot {
    /// Check schema and store generation identity without comparing values or recomputing digests.
    pub(crate) fn same_version(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ir, &other.ir) && Arc::ptr_eq(&self.state, &other.state)
    }

    /// Return the generation captured by this snapshot.
    pub fn ir(&self) -> &Arc<MockIr> {
        &self.ir
    }

    /// Borrow a record by model and external ID.
    pub fn record(&self, model: ModelId, id: &str) -> Option<&Arc<Record>> {
        self.state.record(model, id)
    }

    /// Return total record count and logical byte usage across all models.
    pub fn size(&self) -> (usize, usize) {
        (self.state.usage.records, self.state.usage.bytes)
    }
}

impl State {
    fn record(&self, model: ModelId, id: &str) -> Option<&Arc<Record>> {
        let state = self.models.get(model.0)?;
        state.records.get(state.ids.get(id)?)
    }
}

/// The latest committed state owned by the worker.
pub struct Store {
    current: Snapshot,
}

impl Store {
    /// Create an empty store for every model in the IR.
    pub fn empty(ir: Arc<MockIr>) -> Self {
        let state = State {
            models: (0..ir.models().len()).map(|_| ModelState::new()).collect(),
            relations: vec![Arc::default(); ir.relations().len()],
            next_edge: 0,
            keys: vec![Arc::default(); ir.keys().len()],
            usage: Usage::default(),
        };
        Self {
            current: Snapshot {
                ir,
                state: Arc::new(state),
            },
        }
    }

    /// Fork a private candidate that can be changed and committed without sharing mutable state.
    pub fn snapshot(&self) -> Snapshot {
        self.current.clone()
    }

    /// Commit a prepared transaction after final budget, failure, and staleness checks.
    pub fn commit(&mut self, prepared: Prepared) -> Result<Snapshot, StoreError> {
        prepared.values.work.check()?;
        if prepared.failed {
            return Err(ValueError("output failure prevents commit"));
        }
        if !Arc::ptr_eq(&self.current.state, &prepared.base.state)
            || !Arc::ptr_eq(&self.current.ir, &prepared.base.ir)
        {
            return Err(ValueError("stale transaction"));
        }
        if prepared.write {
            self.current = prepared.result.clone();
        }
        Ok(prepared.result)
    }
}

/// Request-scoped store candidate used by one resolver execution.
pub struct Transaction {
    base: Snapshot,
    state: State,
    scope: CallScope,
    failed: bool,
    values: ValueBudget,
    work: Work,
}

/// Fully checked candidate that the caller may swap inside its cancellation fence.
pub struct Prepared {
    base: Snapshot,
    result: Snapshot,
    write: bool,
    values: ValueBudget,
    failed: bool,
}

impl Prepared {
    /// Return the final validated snapshot for output conversion.
    pub fn snapshot(&self) -> &Snapshot {
        &self.result
    }

    /// Validate a write's return value against the closed candidate using the remaining host-operation budget.
    pub fn output(
        &mut self,
        field: crate::ir::FieldId,
        value: &chigiri::core::Value,
    ) -> Result<crate::output::OutputValue, StoreError> {
        if self.failed {
            return Err(ValueError("output failure prevents execution"));
        }
        let result = crate::output::validate(&self.result, field, value, &mut self.values);
        self.failed |= result.is_err();
        result
    }

    pub(crate) fn output_provenance(
        &mut self,
        field: crate::ir::FieldId,
        value: &chigiri::core::Value,
        provenance: &crate::codec::Provenance,
    ) -> Result<crate::output::OutputValue, crate::output::OutputError> {
        if self.failed {
            return Err(ValueError("output failure prevents execution").into());
        }
        let result = crate::output::validate_provenance_detailed(
            &self.result,
            field,
            value,
            provenance,
            &mut self.values,
        );
        self.failed |= result.is_err();
        result
    }
}

impl Transaction {
    /// Keep VM-budget and return-conversion failures on the store side of commit.
    pub fn fail(&mut self) {
        self.failed = true;
    }

    /// Borrow a record from the candidate for mutation.
    pub fn record(&self, model: ModelId, id: &str) -> Option<&Arc<Record>> {
        self.state.record(model, id)
    }

    /// Validate candidate relations and prepare an immutable result before the caller's commit fence.
    pub fn prepare(mut self) -> Result<Prepared, StoreError> {
        if self.failed {
            return Err(ValueError("host failure prevents commit"));
        }
        self.work.charge(0)?;
        relations::validate(&self.base.ir, &self.state, &mut self.work)?;
        let result = Snapshot {
            ir: self.base.ir.clone(),
            state: Arc::new(self.state),
        };
        Ok(Prepared {
            base: self.base,
            result,
            write: self.scope == CallScope::MutationRoot,
            values: self.values,
            failed: false,
        })
    }

    fn checked<T>(&mut self, result: Result<T, StoreError>) -> Result<T, StoreError> {
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn writable(&self) -> Result<(), StoreError> {
        if self.failed || self.scope != CallScope::MutationRoot {
            Err(ValueError("write capability required"))
        } else {
            Ok(())
        }
    }
}
