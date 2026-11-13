//! Generate candidates, plan ownership, and publish from one exclusive initial snapshot.

use std::path::Path;

use super::filesystem::Directory;
use super::transaction::Point;
use super::update_inputs::Inputs;
use super::{ProjectError, UpdateReport};

/// Place new candidates and ownership metadata from SDL without assuming old resolver or seed readiness.
pub fn update_project(path: &Path) -> Result<UpdateReport, ProjectError> {
    update_with(path, |_| Ok(()))
}

/// Regenerate from the adopted public schema and current project-local inputs.
pub fn codegen_project(path: &Path) -> Result<UpdateReport, ProjectError> {
    codegen_with(path, |_| Ok(()))
}

pub(super) fn update_with(
    path: &Path,
    hook: impl FnMut(Point) -> Result<(), ProjectError>,
) -> Result<UpdateReport, ProjectError> {
    regenerate(path, Authority::External, hook)
}

pub(super) fn codegen_with(
    path: &Path,
    hook: impl FnMut(Point) -> Result<(), ProjectError>,
) -> Result<UpdateReport, ProjectError> {
    regenerate(path, Authority::Adopted, hook)
}

#[derive(Clone, Copy)]
enum Authority {
    External,
    Adopted,
}

fn regenerate(
    path: &Path,
    authority: Authority,
    mut hook: impl FnMut(Point) -> Result<(), ProjectError>,
) -> Result<UpdateReport, ProjectError> {
    let root = Directory::open(path)?;
    let lock = root.lock(true)?;
    root.transaction_absent()?;
    let (schema, old) = match authority {
        Authority::External => super::sources::from_external(root.clone())?,
        Authority::Adopted => super::sources::from_adopted(root.clone())?,
    };
    let next = super::generate::generate_with(
        schema.ir().clone(),
        schema.settings().hooks,
        schema.settings().state.clone(),
        schema.settings().clock.clone(),
        schema.settings().events.clone(),
    )?;
    let mut inputs = Inputs::new(root.clone());
    for input in &schema.files {
        inputs.insert(input.logical.as_str(), input.file.raw());
    }
    let plan = super::update_plan::build(&old, next, inputs)?;
    schema.verify_unchanged()?;
    lock.verify()?;
    root.transaction_absent()?;
    if plan.files.is_empty() {
        return Ok(plan.report);
    }
    super::transaction::run(root, lock, &schema, &old, plan, &mut hook)
}
