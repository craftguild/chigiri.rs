//! Restores rename failures before commit and keeps the complete recovery
//! archive until post-commit cleanup finishes.

use crate::filesystem::{Directory, FsError, RenameFailure};
use crate::{CodegenError, ResolvedManifest};

use super::{Step, Transaction, capture, combine, io};
use crate::contract::write::recovery;

impl Transaction {
    pub(in crate::contract::write) fn install(
        &mut self,
        manifest: &ResolvedManifest,
        hook: &mut impl FnMut(Step) -> Result<(), FsError>,
    ) -> Result<(), CodegenError> {
        crate::cancellation::check()?;
        self.verify_candidate()?;
        crate::contract::marker::verify_inputs(manifest, self.expected.as_ref().unwrap())?;
        manifest.verify_paths()?;
        if let Some((old, expected)) = &self.previous {
            if capture(old)? != *expected {
                return Err(io(FsError::Changed));
            }
            match rename(old, &self.stage, "previous", Step::Backup, hook) {
                Ok(backup) => self.previous.as_mut().unwrap().0 = backup,
                Err(failure) => {
                    let primary = io(failure.error);
                    if let Some(backup) = failure.moved {
                        self.previous.as_mut().unwrap().0 = backup;
                        return Err(self.rollback(primary, false, hook));
                    }
                    return Err(primary);
                }
            }
            let inspected = capture(&self.previous.as_ref().unwrap().0);
            match inspected {
                Ok(tree) if tree == self.previous.as_ref().unwrap().1 => {}
                result => {
                    let primary = result.err().unwrap_or_else(|| io(FsError::Changed));
                    return Err(self.rollback(primary, false, hook));
                }
            }
        }
        let candidate = self.candidate.as_ref().unwrap();
        match rename(candidate, &self.parent, &self.name, Step::Install, hook) {
            Ok(live) => self.candidate = Some(live),
            Err(failure) => {
                let new_live = failure.moved.is_some();
                if let Some(live) = failure.moved {
                    self.candidate = Some(live);
                }
                return Err(self.rollback(io(failure.error), new_live, hook));
            }
        }
        let checked = step(Step::Verify, hook)
            .map_err(io)
            .and_then(|()| self.verify_candidate())
            .and_then(|()| {
                crate::contract::marker::verify_inputs(manifest, self.expected.as_ref().unwrap())
            })
            .and_then(|()| manifest.verify_paths());
        let checked = checked.and_then(|()| crate::cancellation::check());
        if let Err(primary) = checked {
            return Err(self.rollback(primary, true, hook));
        }
        // Keep the new tree installed; rollback failure does not replace the primary error.
        self.committed = true;
        self.candidate = None;
        self.cleanup_after_commit(hook)
    }

    fn rollback(
        &mut self,
        primary: CodegenError,
        new_live: bool,
        hook: &mut impl FnMut(Step) -> Result<(), FsError>,
    ) -> CodegenError {
        crate::cancellation::recovering(|| self.restore(primary, new_live, hook))
    }

    fn restore(
        &mut self,
        mut primary: CodegenError,
        new_live: bool,
        hook: &mut impl FnMut(Step) -> Result<(), FsError>,
    ) -> CodegenError {
        if new_live {
            match rename(
                self.candidate.as_ref().unwrap(),
                &self.stage,
                "candidate",
                Step::Uninstall,
                hook,
            ) {
                Ok(candidate) => self.candidate = Some(candidate),
                Err(failure) => {
                    if let Some(candidate) = failure.moved {
                        self.candidate = Some(candidate);
                    }
                    self.preserve = true;
                    primary = combine(
                        primary,
                        io(failure.error),
                        "The new live candidate could not be moved back during rollback.",
                    );
                }
            }
        }
        if let Some((backup, _)) = &self.previous {
            match rename(backup, &self.parent, &self.name, Step::Rollback, hook) {
                Ok(restored) => self.previous.as_mut().unwrap().0 = restored,
                Err(failure) => {
                    if let Some(restored) = failure.moved {
                        self.previous.as_mut().unwrap().0 = restored;
                    }
                    self.preserve = true;
                    return combine(
                        primary,
                        io(failure.error),
                        "The old generated crate could not be restored during rollback.",
                    );
                }
            }
        }
        primary
    }

    fn cleanup_after_commit(
        &mut self,
        hook: &mut impl FnMut(Step) -> Result<(), FsError>,
    ) -> Result<(), CodegenError> {
        let archive = if let Some((backup, previous)) = &self.previous {
            step(Step::RecoveryWrite, hook).map_err(io)?;
            let archive = recovery::create(&self.parent, &self.name, previous).map_err(io)?;
            archive.verify().map_err(io)?;
            backup
                .remove_tree(previous, &mut || {
                    archive.verify()?;
                    step(Step::CleanupEntry, hook)
                })
                .map_err(io)?;
            self.previous = None;
            Some(archive)
        } else {
            None
        };
        step(Step::StageRemove, hook).map_err(io)?;
        self.cleanup_stage()?;
        if let Some(archive) = archive {
            step(Step::RecoveryRemove, hook).map_err(io)?;
            // Do not add another fallible cleanup after deleting the complete archive of old bytes.
            archive.remove().map_err(io)?;
        }
        Ok(())
    }
}

fn rename(
    source: &Directory,
    parent: &Directory,
    name: &str,
    point: Step,
    hook: &mut impl FnMut(Step) -> Result<(), FsError>,
) -> Result<Directory, RenameFailure> {
    step(point, hook).map_err(|error| RenameFailure { error, moved: None })?;
    source.rename_to(parent, name)
}

fn step(point: Step, hook: &mut impl FnMut(Step) -> Result<(), FsError>) -> Result<(), FsError> {
    crate::cancellation::filesystem()?;
    hook(point)?;
    crate::cancellation::filesystem()
}
