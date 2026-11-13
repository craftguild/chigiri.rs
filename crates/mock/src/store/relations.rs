//! Maintain foreign-key and implicit inverse indexes from the same typed key tuples and validate final state.

use std::{collections::BTreeSet, sync::Arc};

use super::{Record, RelationIndex, State, StoreError, keys, work::Work};
use crate::ir::{KeyId, MockIr, ModelId, RelationStorage};
use crate::value::ValueError;

pub(super) fn replace(
    ir: &MockIr,
    state: &mut State,
    old: Option<&Record>,
    new: Option<&Record>,
    work: &mut Work,
) -> Result<(), StoreError> {
    keys::replace(ir, state, old, new, work)?;
    for (index, relation) in ir.relations().iter().enumerate() {
        work.charge(1)?;
        let model = new
            .or(old)
            .expect("replace requires an old or new record")
            .model;
        match &relation.storage {
            RelationStorage::ForeignKey { slots, key } if relation.owner == model => {
                let targets = |row: Option<&Record>, work: &mut Work| {
                    row.map(|row| keys::references(ir, *key, slots, row, work))
                        .transpose()
                        .map(|values| values.flatten().unwrap_or_default())
                };
                let before = targets(old, work)?;
                let after = targets(new, work)?;
                if let Some(row) = new {
                    let field = &ir.graph().fields()[relation.field.0];
                    if slots
                        .first()
                        .and_then(|slot| row.get(*slot))
                        .is_some_and(|value| !value.is_null())
                    {
                        owner_cardinality(field, after.len())?;
                    }
                }
                let mut seen = BTreeSet::new();
                for target in after.iter().flatten() {
                    if !seen.insert(target.clone()) {
                        return Err(ValueError("duplicate relation reference"));
                    }
                }
                reserve_copy(&state.relations[index], work)?;
                let owner = new
                    .or(old)
                    .expect("replace requires an old or new record")
                    .order;
                for target in before.into_iter().flatten() {
                    work.charge(target.cost() + after.len())?;
                    if seen.contains(&target) {
                        continue;
                    }
                    let entries = Arc::make_mut(&mut state.relations[index]);
                    if let Some(owners) = entries.get_mut(&target) {
                        owners.remove(&owner);
                        if owners.is_empty() {
                            entries.remove(&target);
                        }
                    }
                }
                for target in after.into_iter().flatten() {
                    work.charge(target.cost() + 1)?;
                    if state.relations[index]
                        .get(&target)
                        .is_some_and(|owners| owners.contains_key(&owner))
                    {
                        continue;
                    }
                    let order = next_edge(state)?;
                    Arc::make_mut(&mut state.relations[index])
                        .entry(target)
                        .or_default()
                        .insert(owner, order);
                }
            }
            RelationStorage::Implicit { .. } if new.is_none() => {
                super::implicit::remove_row(
                    ir,
                    state,
                    crate::ir::RelationId(index),
                    old.expect("removal requires the prior record"),
                    work,
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Allocate an edge order owned by candidate state so rollback discards it with the candidate.
pub(super) fn next_edge(state: &mut State) -> Result<u64, StoreError> {
    let order = state.next_edge;
    state.next_edge = order
        .checked_add(1)
        .ok_or(ValueError("edge order exhausted"))?;
    Ok(order)
}

pub(super) fn owner_cardinality(
    field: &crate::ir::FieldNode,
    count: usize,
) -> Result<(), StoreError> {
    if field.constraints.min_items.is_some_and(|min| count < min)
        || field.constraints.max_items.is_some_and(|max| count > max)
    {
        return Err(ValueError("owner relation cardinality"));
    }
    Ok(())
}

pub(super) fn reserve_copy(index: &Arc<RelationIndex>, work: &mut Work) -> Result<(), StoreError> {
    if Arc::strong_count(index) > 1 {
        for (key, owners) in index.iter() {
            work.charge(key.cost() + owners.len())?;
        }
    }
    Ok(())
}

pub(super) fn target_key(storage: &RelationStorage) -> KeyId {
    match storage {
        RelationStorage::ForeignKey { key, .. } => *key,
        RelationStorage::Implicit { target_key, .. } => *target_key,
    }
}

pub(super) fn validate(ir: &MockIr, state: &State, work: &mut Work) -> Result<(), StoreError> {
    validate_detailed(ir, state, work).map_err(|failure| failure.error)
}

pub(super) struct Location {
    pub model: ModelId,
    pub order: u64,
    pub field: Option<Arc<str>>,
}

pub(super) struct Failure {
    pub location: Option<Location>,
    pub error: StoreError,
}

pub(super) fn validate_detailed(
    ir: &MockIr,
    state: &State,
    work: &mut Work,
) -> Result<(), Failure> {
    let mut location = None;
    validate_at(ir, state, work, &mut location).map_err(|error| Failure { location, error })
}

fn validate_at(
    ir: &MockIr,
    state: &State,
    work: &mut Work,
    location: &mut Option<Location>,
) -> Result<(), StoreError> {
    for (index, relation) in ir.relations().iter().enumerate() {
        let key = target_key(&relation.storage);
        for (target, owners) in state.relations[index].iter() {
            work.charge(target.cost() + owners.len())?;
            *location = owners.first_key_value().map(|(order, _)| Location {
                model: relation.owner,
                order: *order,
                field: Some(ir.graph().fields()[relation.field.0].name.clone()),
            });
            if !state.keys[key.0].contains_key(target) {
                return Err(ValueError("unresolved relation reference"));
            }
            if relation.max_refs.is_some_and(|max| owners.len() > max) {
                return Err(ValueError("inverse cardinality exceeded"));
            }
            if owners.len() > crate::value::MAX_ITEMS {
                return Err(ValueError("relation item capacity"));
            }
        }
        if relation.min_refs != 0 {
            for row in state.models[relation.target.0].records.values() {
                *location = Some(Location {
                    model: row.model,
                    order: row.order,
                    field: relation
                        .inverses
                        .first()
                        .map(|field| ir.graph().fields()[field.0].name.clone()),
                });
                let key = keys::record(ir, key, row, work)?;
                let count = state.relations[index]
                    .get(&key)
                    .map_or(0, std::collections::BTreeMap::len);
                if count < relation.min_refs {
                    return Err(ValueError("required inverse missing"));
                }
            }
        }
        let field = &ir.graph().fields()[relation.field.0];
        for row in state.models[relation.owner.0].records.values() {
            *location = Some(Location {
                model: row.model,
                order: row.order,
                field: Some(field.name.clone()),
            });
            let count = match &relation.storage {
                RelationStorage::ForeignKey { slots, key } => {
                    keys::references(ir, *key, slots, row, work)?.map(|values| values.len())
                }
                RelationStorage::Implicit { .. } => {
                    work.charge(state.relations[index].len())?;
                    Some(
                        state.relations[index]
                            .values()
                            .filter(|owners| owners.contains_key(&row.order))
                            .count(),
                    )
                }
            };
            if let Some(count) = count {
                if count > crate::value::MAX_ITEMS
                    || field.constraints.min_items.is_some_and(|min| count < min)
                    || field.constraints.max_items.is_some_and(|max| count > max)
                {
                    return Err(ValueError("owner relation cardinality"));
                }
            } else if !field.ty.nullable() {
                return Err(ValueError("required owner relation missing"));
            }
        }
    }
    Ok(())
}
