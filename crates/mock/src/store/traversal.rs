//! Traverse relations through current rows or sealed captured record slots.

use std::{collections::BTreeSet, sync::Arc};

use super::{Record, Snapshot, State, StoreError, Transaction, keys, relations, work::Work};
use crate::ir::{FieldId, FieldPlan, MockIr, Relation, RelationId, RelationStorage};
use crate::output::CapturedRecord;
use crate::value::ValueError;

/// Result of traversing one schema relation while retaining record wrappers.
#[derive(Debug)]
pub enum Related {
    /// Null result for a nullable singular relation or nullable list.
    Null,
    /// Record returned by a singular relation.
    One(Arc<Record>),
    /// Ordered list result whose nullable items are represented by `None`.
    Many(Vec<Option<Arc<Record>>>),
}

impl Snapshot {
    /// Resolve an owner or inverse relation from this snapshot.
    pub fn related(&self, field: FieldId, parent: &Record) -> Result<Related, StoreError> {
        read(&self.ir, &self.state, field, parent, &mut Work::default())
    }
}

impl Transaction {
    /// Traverse one relation step in candidate state.
    pub fn related(&mut self, field: FieldId, parent: &Record) -> Result<Related, StoreError> {
        let result = if self.failed {
            Err(ValueError("host failure prevents execution"))
        } else {
            read(&self.base.ir, &self.state, field, parent, &mut self.work)
        };
        self.checked(result)
    }
}

pub(super) fn read(
    ir: &MockIr,
    state: &State,
    field: FieldId,
    parent: &Record,
    work: &mut Work,
) -> Result<Related, StoreError> {
    work.charge(1)?;
    let model = field_model(ir, field)?;
    if model != parent.model
        || !parent.belongs_to(ir)
        || !state.models[model.0]
            .records
            .get(&parent.order)
            .is_some_and(|current| std::ptr::eq(current.as_ref(), parent))
    {
        return Err(ValueError("relation parent is not from this snapshot"));
    }
    read_row(ir, state, field, parent.order, work)
}

pub(super) fn read_row(
    ir: &MockIr,
    state: &State,
    field: FieldId,
    parent: u64,
    work: &mut Work,
) -> Result<Related, StoreError> {
    work.charge(1)?;
    let model = field_model(ir, field)?;
    let row = state.models[model.0]
        .records
        .get(&parent)
        .ok_or(ValueError("relation parent missing"))?;
    let (id, inverse) = relation_plan(ir, field)?;
    let relation = &ir.relations()[id.0];
    let rows = if inverse {
        let target = keys::record(ir, relations::target_key(&relation.storage), row, work)?;
        inverse_rows(state, relation, id, &target, work)?
    } else if let RelationStorage::ForeignKey { slots, key } = &relation.storage {
        let Some(targets) = keys::references(ir, *key, slots, row, work)? else {
            return Ok(Related::Null);
        };
        foreign_rows(state, relation, *key, targets, work)?
    } else {
        implicit_rows(state, relation, id, parent, work)?
    };
    shape(ir, field, rows)
}

pub(super) fn read_captured(
    ir: &MockIr,
    state: &State,
    field: FieldId,
    parent: &CapturedRecord,
    work: &mut Work,
) -> Result<Related, StoreError> {
    work.charge(1)?;
    let model = field_model(ir, field)?;
    if model != parent.model() || !parent.belongs_to(ir) {
        return Err(ValueError(
            "captured relation parent is from another schema",
        ));
    }
    let (id, inverse) = relation_plan(ir, field)?;
    let relation = &ir.relations()[id.0];
    let rows = if inverse {
        let target =
            keys::captured_record(ir, relations::target_key(&relation.storage), parent, work)?;
        inverse_rows(state, relation, id, &target, work)?
    } else if let RelationStorage::ForeignKey { slots, key } = &relation.storage {
        let Some(targets) = keys::captured_references(ir, *key, slots, parent, work)? else {
            return Ok(Related::Null);
        };
        foreign_rows(state, relation, *key, targets, work)?
    } else if let RelationStorage::Implicit { owner_key, .. } = relation.storage {
        let owner = keys::captured_record(ir, owner_key, parent, work)?;
        match state.keys[owner_key.0].get(&owner).copied() {
            Some(parent) => implicit_rows(state, relation, id, parent, work)?,
            None => Vec::new(),
        }
    } else {
        unreachable!()
    };
    shape(ir, field, rows)
}

fn field_model(ir: &MockIr, field: FieldId) -> Result<crate::ir::ModelId, StoreError> {
    let definition = ir
        .graph()
        .fields()
        .get(field.0)
        .ok_or(ValueError("unknown relation field"))?;
    ir.model_for(definition.parent)
        .ok_or(ValueError("relation model required"))
}

fn relation_plan(ir: &MockIr, field: FieldId) -> Result<(RelationId, bool), StoreError> {
    match ir.field_plan(field) {
        Some(FieldPlan::Owner(id)) => Ok((*id, false)),
        Some(FieldPlan::Inverse { relation, .. }) => Ok((*relation, true)),
        _ => Err(ValueError("relation field required")),
    }
}

fn inverse_rows(
    state: &State,
    relation: &Relation,
    id: RelationId,
    target: &keys::KeyValue,
    work: &mut Work,
) -> Result<Vec<Option<Arc<Record>>>, StoreError> {
    let owners = state.relations[id.0].get(target);
    work.charge(owners.map_or(0, std::collections::BTreeMap::len))?;
    let mut ordered = owners
        .into_iter()
        .flat_map(|owners| owners.iter())
        .collect::<Vec<_>>();
    work.charge(ordered.len().saturating_mul(usize::BITS as usize))?;
    ordered.sort_unstable_by_key(|(_, order)| **order);
    ordered
        .into_iter()
        .map(|(order, _)| {
            state.models[relation.owner.0]
                .records
                .get(order)
                .cloned()
                .map(Some)
                .ok_or(ValueError("inverse index mismatch"))
        })
        .collect()
}

fn foreign_rows(
    state: &State,
    relation: &Relation,
    key: crate::ir::KeyId,
    targets: Vec<Option<keys::KeyValue>>,
    work: &mut Work,
) -> Result<Vec<Option<Arc<Record>>>, StoreError> {
    work.charge(targets.len())?;
    let mut seen = BTreeSet::new();
    targets
        .into_iter()
        .map(|target| {
            let Some(target) = target else {
                return Ok(None);
            };
            if !seen.insert(target.clone()) {
                return Err(ValueError("duplicate relation reference"));
            }
            let order = state.keys[key.0]
                .get(&target)
                .ok_or(ValueError("unresolved relation reference"))?;
            state.models[relation.target.0]
                .records
                .get(order)
                .cloned()
                .map(Some)
                .ok_or(ValueError("target index mismatch"))
        })
        .collect()
}

fn implicit_rows(
    state: &State,
    relation: &Relation,
    id: RelationId,
    parent: u64,
    work: &mut Work,
) -> Result<Vec<Option<Arc<Record>>>, StoreError> {
    let key = relations::target_key(&relation.storage);
    let mut targets = BTreeSet::new();
    for (target, owners) in state.relations[id.0].iter() {
        work.charge(target.cost() + 1)?;
        if let Some(order) = owners.get(&parent) {
            targets.insert((
                *order,
                *state.keys[key.0]
                    .get(target)
                    .ok_or(ValueError("unresolved relation reference"))?,
            ));
        }
    }
    targets
        .into_iter()
        .map(|(_, order)| {
            state.models[relation.target.0]
                .records
                .get(&order)
                .cloned()
                .map(Some)
                .ok_or(ValueError("target index mismatch"))
        })
        .collect()
}

fn shape(
    ir: &MockIr,
    field: FieldId,
    rows: Vec<Option<Arc<Record>>>,
) -> Result<Related, StoreError> {
    let definition = &ir.graph().fields()[field.0];
    let count = rows.len();
    if count > crate::value::MAX_ITEMS
        || definition
            .constraints
            .min_items
            .is_some_and(|min| count < min)
        || definition
            .constraints
            .max_items
            .is_some_and(|max| count > max)
    {
        return Err(ValueError("relation result cardinality"));
    }
    if definition.ty.list_depth() != 0 {
        return Ok(Related::Many(rows));
    }
    match count {
        0 if definition.ty.nullable() => Ok(Related::Null),
        1 => rows
            .into_iter()
            .next()
            .flatten()
            .map(Related::One)
            .ok_or(ValueError("singular relation key required")),
        _ => Err(ValueError("singular relation cardinality")),
    }
}
