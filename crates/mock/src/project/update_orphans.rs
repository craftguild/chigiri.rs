//! Reject missing active files, clean metadata for deleted orphans, and leave seed and orphan contents unparsed.

use super::state::{Resolver, Seed, State};
use super::update_inputs::{Inputs, failure, missing};
use super::{ProjectError, UpdateAction, UpdateReport};

pub(super) fn retain(
    old: &State,
    next: &State,
    inputs: &mut Inputs,
    report: &mut UpdateReport,
) -> Result<(Vec<Resolver>, Vec<Seed>), ProjectError> {
    let mut resolvers = Vec::new();
    for entry in &old.resolvers.0 {
        let size = inputs.size(&entry.path)?;
        if entry.active && size.is_none() {
            return Err(missing(&entry.path));
        }
        if entry.active && size.is_some_and(|n| n > crate::luau::MAX_BODY as u64) {
            return Err(failure(&entry.path, "limit", "module source byte limit"));
        }
        let dist_present = if let Some(dist) = &entry.dist {
            inputs.observe(&dist.path)?.exists()
        } else {
            false
        };
        if entry.active
            && next
                .resolvers
                .0
                .iter()
                .any(|new| new.coordinate == entry.coordinate && new.path == entry.path)
        {
            continue;
        }
        if !entry.active && size.is_none() && !dist_present {
            report.event(UpdateAction::OrphanRemoved, &entry.path);
        } else {
            let mut orphan = entry.clone();
            orphan.active = false;
            resolvers.push(orphan);
            report.event(UpdateAction::Orphaned, &entry.path);
        }
    }
    let mut seeds = Vec::new();
    let mut remaining = crate::limits::SEED_BYTES;
    for entry in &old.seeds.0 {
        let size = inputs.size(&entry.path)?;
        if entry.active {
            let size = size.ok_or_else(|| missing(&entry.path))?;
            if size > remaining as u64 {
                return Err(failure(&entry.path, "limit", "aggregate JSONL byte limit"));
            }
            remaining -= size as usize;
        }
        if entry.active
            && next
                .seeds
                .0
                .iter()
                .any(|new| new.model == entry.model && new.path == entry.path)
        {
            continue;
        }
        if !entry.active && size.is_none() {
            report.event(UpdateAction::OrphanRemoved, &entry.path);
        } else {
            let mut orphan = entry.clone();
            orphan.active = false;
            seeds.push(orphan);
            report.event(UpdateAction::Orphaned, &entry.path);
        }
    }
    Ok((resolvers, seeds))
}
