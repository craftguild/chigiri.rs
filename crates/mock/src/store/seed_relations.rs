//! Resolve explicit seed relations after every record is loaded and reject contradictions across either side.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
};

use chigiri::core::{Object, Value};

use super::{State, StoreError, key_reference, keys::KeyValue, relations};
use crate::ir::{FieldId, FieldPlan, MockIr, ModelId, RelationId, RelationStorage};
use crate::value::{ValueBudget, ValueError};

pub(super) struct Pending {
    relation: RelationId,
    inverse: bool,
    row: u64,
    references: Vec<KeyValue>,
    pub model: ModelId,
    pub field: FieldId,
}

pub(super) fn normalize<'a>(
    ir: &MockIr,
    model: ModelId,
    order: u64,
    row: &'a Object,
    pending: &mut Vec<Pending>,
    budget: &mut ValueBudget,
) -> Result<Cow<'a, Object>, StoreError> {
    let mut normalized = Cow::Borrowed(row);
    let node = &ir.graph().types()[ir.models()[model.0].ty.0];
    for (name, value) in row {
        budget.work.charge(1 + name.len())?;
        let Some(field) = node.fields.get(name).copied() else {
            continue;
        };
        let (relation, inverse) = match ir.field_plan(field) {
            Some(FieldPlan::Owner(id)) => (*id, false),
            Some(FieldPlan::Inverse { relation, .. }) => (*relation, true),
            _ => continue,
        };
        let definition = &ir.relations()[relation.0];
        if let Cow::Borrowed(_) = normalized {
            budget.container(1)?;
            for (name, value) in row {
                budget.key(name)?;
                budget.reserve_value(value, 2)?;
            }
        }
        normalized.to_mut().remove(name);
        match &definition.storage {
            RelationStorage::Implicit {
                owner_key,
                target_key,
            } => {
                let Value::List(values) = value else {
                    return Err(ValueError("implicit seed membership requires an array"));
                };
                budget.items(values.len())?;
                let key = if inverse { *owner_key } else { *target_key };
                let mut references = Vec::new();
                let mut seen = BTreeSet::new();
                for value in values {
                    let key = key_reference::parse(ir, key, value, budget)?
                        .ok_or(ValueError("implicit reference cannot be null"))?;
                    if !seen.insert(key.clone()) {
                        return Err(ValueError("duplicate implicit seed reference"));
                    }
                    references.push(key);
                }
                pending.push(Pending {
                    relation,
                    inverse,
                    row: order,
                    references,
                    model,
                    field,
                });
            }
            RelationStorage::ForeignKey { slots, key } if !inverse => {
                let list = ir.graph().fields()[field.0].ty.list_depth() != 0;
                let references = match value {
                    Value::Null => None,
                    Value::List(values) if list => {
                        budget.items(values.len())?;
                        Some(
                            values
                                .iter()
                                .map(|value| key_reference::parse(ir, *key, value, budget))
                                .collect::<Result<Vec<_>, _>>()?,
                        )
                    }
                    _ if !list => Some(vec![key_reference::parse(ir, *key, value, budget)?]),
                    _ => return Err(ValueError("relation seed reference array required")),
                };
                for (position, slot) in slots.iter().enumerate() {
                    let name = &ir.slots()[slot.0].name;
                    if row.contains_key(name) {
                        return Err(ValueError("seed supplies relation and FK component"));
                    }
                    budget.key(name)?;
                    match &references {
                        None => budget.node(&Value::Null, 2)?,
                        Some(values) if list => {
                            budget.container(2)?;
                            budget.items(values.len())?;
                            for key in values {
                                if let Some(key) = key {
                                    key.reserve_component(position, budget, 3)?;
                                } else {
                                    budget.node(&Value::Null, 3)?;
                                }
                            }
                        }
                        Some(values) => {
                            if let Some(Some(key)) = values.first() {
                                key.reserve_component(position, budget, 2)?;
                            } else {
                                budget.node(&Value::Null, 2)?;
                            }
                        }
                    }
                    let value = match &references {
                        None => Value::Null,
                        Some(values) => {
                            let values = values
                                .iter()
                                .map(|key| {
                                    key.as_ref()
                                        .and_then(|key| key.value(position))
                                        .unwrap_or(Value::Null)
                                })
                                .collect::<Vec<_>>();
                            if list {
                                Value::List(values)
                            } else {
                                values.into_iter().next().unwrap_or(Value::Null)
                            }
                        }
                    };
                    normalized.to_mut().insert(name.to_string(), value);
                }
            }
            _ => return Err(ValueError("inverse seed field is computed")),
        }
    }
    Ok(normalized)
}

pub(super) fn resolve(
    ir: &MockIr,
    state: &mut State,
    pending: &[Pending],
    budget: &mut ValueBudget,
) -> Result<(), relations::Failure> {
    let mut location = None;
    let result = resolve_at(ir, state, pending, budget, &mut location);
    result.map_err(|error| relations::Failure { location, error })
}

fn resolve_at(
    ir: &MockIr,
    state: &mut State,
    pending: &[Pending],
    budget: &mut ValueBudget,
    location: &mut Option<relations::Location>,
) -> Result<(), StoreError> {
    let mut declared: BTreeMap<(usize, bool, u64), BTreeSet<u64>> = BTreeMap::new();
    for declaration in pending {
        *location = Some(relations::Location {
            model: declaration.model,
            order: declaration.row,
            field: Some(ir.graph().fields()[declaration.field.0].name.clone()),
        });
        let relation = &ir.relations()[declaration.relation.0];
        let RelationStorage::Implicit {
            owner_key,
            target_key,
        } = relation.storage
        else {
            return Err(ValueError("implicit seed plan required"));
        };
        let key = if declaration.inverse {
            owner_key
        } else {
            target_key
        };
        let mut rows = BTreeSet::new();
        for reference in &declaration.references {
            budget.work.charge(reference.cost() + 1)?;
            let row = state.keys[key.0]
                .get(reference)
                .copied()
                .ok_or(ValueError("unresolved implicit seed reference"))?;
            rows.insert(row);
        }
        declared.insert(
            (declaration.relation.0, declaration.inverse, declaration.row),
            rows,
        );
    }
    for ((index, inverse, row), targets) in &declared {
        let relation = &ir.relations()[*index];
        for target in targets {
            budget.work.charge(1)?;
            if declared
                .get(&(*index, !inverse, *target))
                .is_some_and(|other| !other.contains(row))
            {
                return Err(ValueError("conflicting implicit seed memberships"));
            }
            let (owner, target) = if *inverse {
                (*target, *row)
            } else {
                (*row, *target)
            };
            let target_record = &state.models[relation.target.0].records[&target];
            let key = super::keys::record(
                ir,
                relations::target_key(&relation.storage),
                target_record,
                &mut budget.work,
            )?;
            super::implicit::set(state, RelationId(*index), owner, key, true, budget)?;
        }
    }
    Ok(())
}
