//! Retain candidate and prior versions until commit, restoring every item after a pre-commit failure.

mod cleanup;
mod install;
mod parents;
mod prepare;
mod recovery;

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::sync::Arc;

use super::digest::Digest;
use super::filesystem::{Directory, DirectoryLock, FileSnapshot, RawSnapshot};
use super::management::Management;
use super::update_plan::Plan;
use super::{ProjectError, ProjectSchema, UpdateReport, metadata};
use chigiri_host::PortableRelativePath;

/// The test injects IO failures/simultaneous editing at this boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Point {
    CandidateCreated(usize),
    Prepared,
    BeforeBackup(usize),
    AfterBackup(usize),
    BeforeInstall(usize),
    AfterInstall(usize),
    BeforeCommit,
    CommitReady,
    Committed,
    BeforeRestore(usize),
    AfterRestore(usize),
    BeforeCleanup(usize),
    AfterCleanup(usize),
}

struct Operation {
    path: PortableRelativePath,
    parent: Option<PortableRelativePath>,
    name: OsString,
    original: Option<Arc<RawSnapshot>>,
    candidate: Option<FileSnapshot>,
    backup: Option<RawSnapshot>,
    installed: Option<FileSnapshot>,
    restored: Option<RawSnapshot>,
}

struct Prepared {
    root: Directory,
    manifest: Option<FileSnapshot>,
    operations: Vec<Operation>,
    parents: parents::Parents,
    recovery_budget: usize,
    old_generation: Digest,
    new_generation: Digest,
}

pub(super) fn run(
    root: Directory,
    lock: DirectoryLock,
    schema: &ProjectSchema,
    old: &Management,
    plan: Plan,
    hook: &mut impl FnMut(Point) -> Result<(), ProjectError>,
) -> Result<UpdateReport, ProjectError> {
    run_with_commit(root, lock, schema, old, plan, hook, || Ok(()))
}

pub(super) fn run_with_commit(
    root: Directory,
    lock: DirectoryLock,
    schema: &ProjectSchema,
    old: &Management,
    mut plan: Plan,
    hook: &mut impl FnMut(Point) -> Result<(), ProjectError>,
    decide: impl FnOnce() -> Result<(), ProjectError>,
) -> Result<UpdateReport, ProjectError> {
    let changed: BTreeSet<_> = plan.files.keys().cloned().collect();
    plan.inputs.reject_shared_targets(&changed)?;
    let old_bytes: usize = old.files.iter().map(|file| file.bytes.len()).sum();
    let auxiliary_budget = metadata::MAX_BYTES.saturating_sub(old_bytes.max(plan.metadata_bytes));
    let mut prepared = prepare::create(
        &root,
        &mut plan,
        &old.state.generation,
        auxiliary_budget,
        hook,
    )?;
    let result: Result<(), ProjectError> = (|| {
        hook(Point::Prepared)?;
        plan.inputs.verify()?;
        schema.verify_inputs_unchanged()?;
        lock.verify()?;
        prepared.parents.verify_before()?;
        prepared.verify_stage()?;
        install::apply(&mut prepared, hook)?;
        hook(Point::BeforeCommit)?;
        prepared.verify_installed()?;
        plan.inputs.verify_untouched(&changed)?;
        schema.verify_inputs_unchanged()?;
        lock.verify()?;
        hook(Point::CommitReady)?;
        // Cross the logical commit point only after the final I/O verification.
        decide()
    })();
    if let Err(mut error) = result {
        install::rollback(&mut prepared, hook, &mut error.recovery);
        prepared.parents.rollback(&mut error.recovery);
        if error.recovery.is_empty()
            && let Err(failure) = prepared.cleanup(hook)
        {
            error.recovery.push(failure);
        }
        if !error.recovery.is_empty() {
            prepared.record_failure(&mut error);
        }
        return Err(error);
    }
    // All candidate bytes and management generations are committed at this point.
    let cleanup = hook(Point::Committed).and_then(|()| prepared.cleanup(hook));
    if let Err(mut error) = cleanup {
        error.committed = true;
        prepared.record_failure(&mut error);
        return Err(error);
    }
    Ok(plan.report)
}

fn failure(path: &str, phase: &'static str, message: &'static str) -> ProjectError {
    super::update_inputs::failure(path, phase, message)
}
