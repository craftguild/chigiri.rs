//! Build a placement plan from newly generated candidate bytes.

use std::collections::BTreeMap;

use super::digest::Digest;
use super::management::Management;
use super::update_inputs::{Inputs, failure};
use super::{GeneratedProject, ProjectError, UpdateAction, UpdateReport};
use chigiri_host::PortableRelativePath;

pub(super) struct Plan {
    pub files: BTreeMap<PortableRelativePath, String>,
    pub inputs: Inputs,
    pub report: UpdateReport,
    pub generation: Digest,
    pub metadata_bytes: usize,
}

pub(super) fn build(
    old: &Management,
    mut next: GeneratedProject,
    mut inputs: Inputs,
) -> Result<Plan, ProjectError> {
    for (path, file) in [
        "generated/types.d.luau",
        "generated/schema.snapshot.json",
        "generated/update-state.json",
    ]
    .into_iter()
    .zip(&old.files)
    {
        inputs.insert(path, file.raw());
    }
    let mut report = UpdateReport {
        schema_changed: old.state.schema_digest != next.state.schema_digest
            || old.state.public_digest != next.state.public_digest,
        ..UpdateReport::default()
    };
    report.compare(&old.snapshot.summary, &next.snapshot.summary);
    super::update_consumers::reconcile(&old.state, &mut next, &mut inputs, &mut report)?;
    let (orphan_resolvers, orphan_seeds) =
        super::update_orphans::retain(&old.state, &next.state, &mut inputs, &mut report)?;
    for entry in &mut next.state.resolvers.0 {
        if let Some(prior) = old.state.resolvers.0.iter().find(|prior| {
            prior.active && prior.coordinate == entry.coordinate && prior.path == entry.path
        }) {
            let revision = super::update_modules::reconcile(
                &entry.path,
                &prior.base,
                prior.dist.as_ref(),
                &mut next.files,
                &mut inputs,
                &mut report,
            )?;
            entry.base = revision.base;
            entry.candidate = revision.candidate;
            entry.dist = revision.dist;
        } else {
            if orphan_resolvers
                .iter()
                .any(|orphan| orphan.path == entry.path)
                || inputs.observe(&entry.path)?.exists()
            {
                return Err(collision(&entry.path));
            }
            report.event(UpdateAction::Created, &entry.path);
        }
    }
    for entry in &mut next.state.hooks.0 {
        let prior = old
            .state
            .hooks
            .0
            .iter()
            .find(|old| old.slot == entry.slot)
            .ok_or_else(|| collision(&entry.path))?;
        let revision = super::update_modules::reconcile(
            &entry.path,
            &prior.base,
            prior.dist.as_ref(),
            &mut next.files,
            &mut inputs,
            &mut report,
        )?;
        entry.base = revision.base;
        entry.candidate = revision.candidate;
        entry.dist = revision.dist;
    }
    for entry in &next.state.seeds.0 {
        if old
            .state
            .seeds
            .0
            .iter()
            .any(|prior| prior.active && prior.model == entry.model && prior.path == entry.path)
        {
            next.files.remove(&entry.path);
            report.event(UpdateAction::SeedKept, &entry.path);
        } else {
            if orphan_seeds.iter().any(|orphan| orphan.path == entry.path)
                || inputs.observe(&entry.path)?.exists()
            {
                return Err(collision(&entry.path));
            }
            report.event(UpdateAction::SeedCreated, &entry.path);
        }
    }
    next.state.resolvers.0.extend(orphan_resolvers);
    next.state.seeds.0.extend(orphan_seeds);
    let types = &next.files["generated/types.d.luau"];
    let (snapshot, state) = next.state.render_documents(&mut next.snapshot, types)?;
    next.files.insert(
        super::fixed_path("generated/schema.snapshot.json"),
        snapshot,
    );
    next.files
        .insert(super::fixed_path("generated/update-state.json"), state);
    let metadata_bytes = [
        "generated/types.d.luau",
        "generated/schema.snapshot.json",
        "generated/update-state.json",
    ]
    .iter()
    .map(|path| next.files[*path].len())
    .sum();
    for (path, file) in [
        "generated/types.d.luau",
        "generated/schema.snapshot.json",
        "generated/update-state.json",
    ]
    .into_iter()
    .zip(&old.files)
    {
        if next.files.get(path).is_some_and(|new| new == &file.bytes) {
            next.files.remove(path);
        }
    }
    inputs.verify()?;
    Ok(Plan {
        files: next.files,
        inputs,
        report,
        generation: next.state.generation,
        metadata_bytes,
    })
}

fn collision(path: &str) -> ProjectError {
    failure(path, "ownership", "managed or edited destination collision")
}
