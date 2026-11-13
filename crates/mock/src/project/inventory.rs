//! Match management-file paths to the closed layout, rejecting out-of-scope, duplicate, and case-colliding entries.

use std::collections::BTreeSet;

use super::ProjectError;
use super::layout::{Paths, component};
use super::metadata;
use super::state::{Snapshot, State};

pub(super) fn validate(state: &State, snapshot: &Snapshot) -> Result<(), ProjectError> {
    let mut paths = Paths::default();
    let mut active = BTreeSet::new();
    for fixed in [
        "mock.luau",
        "generated/types.d.luau",
        "generated/schema.snapshot.json",
        "generated/update-state.json",
    ] {
        paths.insert(&super::fixed_path(fixed))?;
    }
    super::consumer_inventory::validate(state, &mut paths)?;
    for entry in &state.resolvers.0 {
        if !metadata::coordinate(&entry.coordinate) {
            return Err(invalid());
        }
        let (owner, field) = entry.coordinate.split_once('.').ok_or_else(invalid)?;
        component(field, ".luau.dist".len())?;
        let query = entry.path.as_str() == format!("resolvers/query/{field}.luau");
        let mutation = entry.path.as_str() == format!("resolvers/mutation/{field}.luau");
        let subscription = entry.path.as_str() == format!("resolvers/subscription/{field}.luau");
        let object = entry.path.as_str() == format!("resolvers/objects/{owner}/{field}.luau");
        if object {
            component(owner, 0)?;
        }
        if !(query || mutation || subscription || object) {
            return Err(invalid());
        }
        if entry.active
            && (!active.insert(&entry.coordinate)
                || (query && owner != snapshot.summary.query)
                || (mutation && Some(owner) != snapshot.summary.mutation.as_deref())
                || (subscription && Some(owner) != snapshot.summary.subscription.as_deref())
                || (object
                    && (owner == snapshot.summary.query
                        || Some(owner) == snapshot.summary.mutation.as_deref()
                        || Some(owner) == snapshot.summary.subscription.as_deref())))
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
    for seed in &state.seeds.0 {
        if !metadata::name(&seed.model)
            || seed.path.as_str() != format!("seeds/{}.jsonl", seed.model)
        {
            return Err(invalid());
        }
        component(&seed.model, ".jsonl".len())?;
        paths.insert(&seed.path)?;
    }
    if state.hooks.0.len() != crate::hooks::HookSlot::ALL.len() {
        return Err(invalid());
    }
    let mut slots = BTreeSet::new();
    for entry in &state.hooks.0 {
        if !slots.insert(entry.slot) || entry.path != entry.slot.path() {
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
    Ok(())
}

fn invalid() -> ProjectError {
    ProjectError::new("ownership", "invalid managed file inventory")
}
