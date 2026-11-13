//! Stage the manifest and all candidates privately, retaining original snapshots for recovery.

use serde::Serialize;
use std::ffi::OsStr;

use super::{Digest, Directory, Operation, Plan, Point, Prepared, ProjectError, metadata};
use crate::project::filesystem::TRANSACTION;

#[derive(Serialize)]
struct Manifest<'a> {
    format: &'static str,
    old_generation: &'a Digest,
    new_generation: &'a Digest,
    operations: Vec<Entry<'a>>,
}

#[derive(Serialize)]
struct Entry<'a> {
    path: &'a str,
    candidate: String,
    backup: String,
    original: Option<Digest>,
    next: Digest,
}

pub(super) fn create(
    root: &Directory,
    plan: &mut Plan,
    old_generation: &Digest,
    budget: usize,
    hook: &mut impl FnMut(Point) -> Result<(), ProjectError>,
) -> Result<Prepared, ProjectError> {
    let parents = super::parents::Parents::capture(root, plan.files.keys())?;
    let mut operations = Vec::new();
    for path in plan.files.keys() {
        let original = plan.inputs.read(path, metadata::MAX_BYTES)?;
        operations.push(Operation {
            path: path.clone(),
            parent: path.parent(),
            name: path.file_name().into(),
            original,
            candidate: None,
            backup: None,
            installed: None,
            restored: None,
        });
    }
    let manifest = Manifest {
        format: "chigiri-mock-transaction/v1",
        old_generation,
        new_generation: &plan.generation,
        operations: operations
            .iter()
            .enumerate()
            .map(|(index, operation)| Entry {
                path: operation.path.as_str(),
                candidate: format!("candidate-{index}"),
                backup: format!("backup-{index}"),
                original: operation
                    .original
                    .as_ref()
                    .map(|file| Digest::bytes(&file.bytes)),
                next: Digest::bytes(plan.files[&operation.path].as_bytes()),
            })
            .collect(),
    };
    let manifest = metadata::render(&manifest, budget.min(1024 * 1024))?;
    plan.inputs.verify()?;
    let stage = root
        .create_directory_observed(OsStr::new(TRANSACTION))
        .map_err(|failure| {
            let mut error = *failure.error;
            error.set_file(TRANSACTION);
            if failure.created {
                error.recovery.push(super::failure(
                    TRANSACTION,
                    "recovery",
                    "transaction directory creation could not be verified",
                ));
            }
            error
        })?;
    let mut prepared = Prepared {
        root: stage,
        manifest: None,
        operations,
        parents,
        recovery_budget: (budget - manifest.len()).min(4 * 1024 * 1024),
        old_generation: old_generation.clone(),
        new_generation: plan.generation.clone(),
    };
    let result = (|| {
        prepared.manifest = Some(
            prepared
                .root
                .create_file(OsStr::new("manifest.json"), &manifest)?,
        );
        for (index, operation) in prepared.operations.iter_mut().enumerate() {
            let name = format!("candidate-{index}");
            operation.candidate = Some(
                prepared
                    .root
                    .create_file(OsStr::new(&name), &plan.files[&operation.path])?,
            );
            hook(Point::CandidateCreated(index))?;
        }
        prepared.verify_stage()
    })();
    if let Err(mut error) = result {
        if let Err(failure) = prepared.cleanup(hook) {
            error.recovery.push(failure);
        }
        if !error.recovery.is_empty() {
            prepared.record_failure(&mut error);
        }
        return Err(error);
    }
    plan.files.clear();
    Ok(prepared)
}
