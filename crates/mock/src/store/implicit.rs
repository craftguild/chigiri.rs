//! Update authoritative implicit-edge indexes, leaving duplicate connect or disconnect operations as no-ops.

use std::sync::Arc;

use super::{
    Record, State, StoreError, Usage,
    keys::{self, KeyValue},
    relations,
    work::Work,
};
use crate::ir::{MockIr, RelationId};
use crate::value::ValueBudget;

pub(super) fn set(
    state: &mut State,
    relation: RelationId,
    owner: u64,
    target: KeyValue,
    connect: bool,
    budget: &mut ValueBudget,
) -> Result<(), StoreError> {
    let index = &state.relations[relation.0];
    budget.work.charge(target.cost() + 1)?;
    let found = index
        .get(&target)
        .is_some_and(|owners| owners.contains_key(&owner));
    if found == connect {
        return Ok(());
    }
    let edge = usage(&target, 1);
    let capacity = if connect {
        state.usage.replacing(Usage::default(), edge)?
    } else {
        state.usage.replacing(edge, Usage::default())?
    };
    budget.container(1)?;
    budget.scratch(edge.bytes)?;
    relations::reserve_copy(index, &mut budget.work)?;
    let order = if connect {
        Some(relations::next_edge(state)?)
    } else {
        None
    };
    let index = Arc::make_mut(&mut state.relations[relation.0]);
    if connect {
        index.entry(target).or_default().insert(
            owner,
            order.expect("a new edge must have an insertion order"),
        );
    } else if let Some(owners) = index.get_mut(&target) {
        owners.remove(&owner);
        if owners.is_empty() {
            index.remove(&target);
        }
    }
    state.usage = capacity;
    Ok(())
}

pub(super) fn remove_row(
    ir: &MockIr,
    state: &mut State,
    relation: RelationId,
    row: &Record,
    work: &mut Work,
) -> Result<(), StoreError> {
    let definition = &ir.relations()[relation.0];
    if definition.owner != row.model && definition.target != row.model {
        return Ok(());
    }
    let target = if definition.target == row.model {
        Some(keys::record(
            ir,
            relations::target_key(&definition.storage),
            row,
            work,
        )?)
    } else {
        None
    };
    let mut removed = Usage::default();
    for (key, owners) in state.relations[relation.0].iter() {
        work.charge(key.cost() + 1)?;
        let count = if target.as_ref() == Some(key) {
            owners.len()
        } else {
            usize::from(definition.owner == row.model && owners.contains_key(&row.order))
        };
        let edge = usage(key, count);
        removed.nodes += edge.nodes;
        removed.bytes += edge.bytes;
    }
    let capacity = state.usage.replacing(removed, Usage::default())?;
    relations::reserve_copy(&state.relations[relation.0], work)?;
    Arc::make_mut(&mut state.relations[relation.0]).retain(|key, owners| {
        if target.as_ref() == Some(key) {
            return false;
        }
        if definition.owner == row.model {
            owners.remove(&row.order);
        }
        !owners.is_empty()
    });
    state.usage = capacity;
    Ok(())
}

fn usage(key: &KeyValue, count: usize) -> Usage {
    Usage {
        records: 0,
        nodes: count,
        bytes: (key.cost() + 16) * count,
    }
}
