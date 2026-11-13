//! Verify generation stability, then pass captured active bytes to the shared readiness check.

use std::path::Path;

use super::filesystem::Directory;
use super::{ActiveSnapshot, Layout, ProjectError};
use crate::checked::{CheckError, CheckedMock};
use crate::ir::MockIr;

/// Evaluate configuration once and return validated owned bytes without opening proposals or orphans.
pub fn read_project(path: &Path) -> Result<CheckedMock, ProjectError> {
    read_project_observed(path, |_| {})
}

/// Expose the already-built schema IR for diagnostics before readiness validation completes.
pub fn read_project_observed(
    path: &Path,
    observed: impl FnOnce(&MockIr),
) -> Result<CheckedMock, ProjectError> {
    read_observed(path, observed, || {})
}

#[cfg(test)]
pub(super) fn read_with(
    path: &Path,
    after_check: impl FnOnce(),
) -> Result<CheckedMock, ProjectError> {
    read_observed(path, |_| {}, after_check)
}

fn read_observed(
    path: &Path,
    observed: impl FnOnce(&MockIr),
    after_check: impl FnOnce(),
) -> Result<CheckedMock, ProjectError> {
    let directory = Directory::open(path)?;
    let lock = directory.lock(false)?;
    let snapshot = ActiveSnapshot::from_directory(directory)?;
    observed(snapshot.schema().ir());
    let checked = snapshot.check()?;
    after_check();
    // Reread only to detect replacement; validated store and module contents remain authoritative.
    snapshot.verify_unchanged()?;
    lock.verify()?;
    Ok(checked)
}

pub(super) fn check_error(ir: &MockIr, layout: &Layout, error: CheckError) -> ProjectError {
    match error {
        CheckError::Settings(error) => *error,
        CheckError::Script(error) => {
            let mut failure = ProjectError::new(error.phase, error.message);
            failure.cause = error.cause;
            failure.member.clone_from(&error.member);
            failure.expected.clone_from(&error.expected);
            failure.actual.clone_from(&error.actual);
            failure.hint.clone_from(&error.hint);
            failure.consumer.clone_from(&error.consumer);
            if let Some(position) = error.source_position() {
                failure.set_position(position);
            }
            failure.exit = match error.phase {
                "environment" | "timeout" => 2,
                "cancelled" => 130,
                _ => 1,
            };
            if let Some(entry) = error.script.and_then(|id| layout.resolvers.get(id.0)) {
                failure.set_file(entry.path.clone());
                failure.set_coordinate(entry.coordinate.clone());
            }
            if let Some(slot) = error.hook {
                failure.set_file(slot.path());
                failure.set_hook_slot(slot);
            }
            if let Some(consumer) = error.consumer.as_deref().and_then(|name| {
                layout
                    .consumers
                    .iter()
                    .find(|consumer| consumer.name() == name)
            }) {
                failure.set_file(consumer.path().clone());
                failure.set_coordinate(consumer.name());
            }
            failure
        }
        CheckError::Seed(error) => {
            let mut failure = ProjectError::new("seed", error.message);
            failure.set_position((error.line, 1));
            failure.set_file("seeds");
            if let Some(entry) = error.model.and_then(|id| layout.seeds.get(id.0)) {
                failure.set_file(entry.path.clone());
                let model = &ir.graph().types()[ir.models()[entry.model.0].ty.0].name;
                failure.set_coordinate(match error.field {
                    Some(field) => format!("{model}.{}", diagnostic_field(&field)),
                    None => model.to_string(),
                });
            }
            failure
        }
        CheckError::Limit(message) => ProjectError::new("limit", message),
        CheckError::Environment(message) => {
            let mut error = ProjectError::new("environment", message);
            error.exit = 2;
            error
        }
    }
}

fn diagnostic_field(field: &str) -> String {
    // Escape and bound SDL-derived relation keys before including them in diagnostics.
    let mut output: String = field
        .chars()
        .take(128)
        .flat_map(char::escape_default)
        .collect();
    if field.chars().nth(128).is_some() {
        output.push_str("...");
    }
    output
}
