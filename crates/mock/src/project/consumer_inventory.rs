//! Verify registered and orphan consumer ownership against the closed config inventory.

use std::collections::BTreeSet;

use super::layout::Paths;
use super::state::State;
use super::{Layout, ProjectError};
use crate::ir::MockIr;

pub(super) fn validate(state: &State, paths: &mut Paths) -> Result<(), ProjectError> {
    let configured = state.config.events.consumers();
    let mut active = BTreeSet::new();
    for entry in &state.consumers.0 {
        crate::events::validate_name(&entry.name).map_err(|_| invalid())?;
        if !super::metadata::name(&entry.field)
            || entry.path.as_str() != format!("consumers/{}.luau", entry.name)
        {
            return Err(invalid());
        }
        if entry.active
            && (!active.insert(entry.name.as_str())
                || configured
                    .get(&entry.name)
                    .is_none_or(|registration| registration.field != entry.field))
        {
            return Err(invalid());
        }
        paths.insert(&entry.path)?;
        if let Some(dist) = &entry.dist {
            if dist.path.as_str() != format!("{}.dist", entry.path) {
                return Err(invalid());
            }
            paths.insert(&dist.path)?;
        }
    }
    if active.len() != configured.len() {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn current(state: &State, ir: &MockIr, layout: &Layout) -> Result<(), ProjectError> {
    let active: Vec<_> = state
        .consumers
        .0
        .iter()
        .filter(|entry| entry.active)
        .collect();
    if active.len() != layout.consumers.len()
        || layout.consumers.iter().any(|consumer| {
            !active.iter().any(|entry| {
                entry.name == consumer.name()
                    && entry.path == *consumer.path()
                    && entry.field == ir.graph().fields()[consumer.field().0].name.as_ref()
            })
        })
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> ProjectError {
    let mut error = ProjectError::new("ownership", "invalid consumer ownership inventory");
    error.set_file("generated/update-state.json");
    error
}
