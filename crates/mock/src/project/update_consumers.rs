//! Preserve authored registered modules and retain removed modules as owned orphans.

use super::state::State;
use super::update_inputs::{Inputs, failure, missing};
use super::{GeneratedProject, ProjectError, UpdateAction, UpdateReport};

pub(super) fn reconcile(
    old: &State,
    next: &mut GeneratedProject,
    inputs: &mut Inputs,
    report: &mut UpdateReport,
) -> Result<(), ProjectError> {
    let mut orphans = Vec::new();
    for entry in &old.consumers.0 {
        let size = inputs.size(&entry.path)?;
        if entry.active && size.is_none() {
            return Err(missing(&entry.path));
        }
        if entry.active && size.is_some_and(|bytes| bytes > crate::luau::MAX_BODY as u64) {
            return Err(failure(
                &entry.path,
                "limit",
                "consumer module source byte limit",
            ));
        }
        let dist_present = if let Some(dist) = &entry.dist {
            inputs.observe(&dist.path)?.exists()
        } else {
            false
        };
        if entry.active
            && next
                .state
                .consumers
                .0
                .iter()
                .any(|new| new.name == entry.name && new.path == entry.path)
        {
            continue;
        }
        if !entry.active && size.is_none() && !dist_present {
            report.event(UpdateAction::OrphanRemoved, &entry.path);
        } else {
            let mut orphan = entry.clone();
            orphan.active = false;
            orphans.push(orphan);
            report.event(UpdateAction::Orphaned, &entry.path);
        }
    }
    for entry in &mut next.state.consumers.0 {
        if let Some(prior) = old
            .consumers
            .0
            .iter()
            .find(|prior| prior.active && prior.name == entry.name && prior.path == entry.path)
        {
            let revision = super::update_modules::reconcile(
                &entry.path,
                &prior.base,
                prior.dist.as_ref(),
                &mut next.files,
                inputs,
                report,
            )?;
            entry.base = revision.base;
            entry.candidate = revision.candidate;
            entry.dist = revision.dist;
        } else {
            if orphans.iter().any(|orphan| orphan.path == entry.path)
                || inputs.observe(&entry.path)?.exists()
            {
                return Err(failure(
                    &entry.path,
                    "ownership",
                    "managed or edited consumer destination collision",
                ));
            }
            report.event(UpdateAction::Created, &entry.path);
        }
    }
    next.state.consumers.0.extend(orphans);
    Ok(())
}
