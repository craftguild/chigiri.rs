//! Remove verified candidate and backup files individually while preserving unknown or modified recovery evidence.

use std::collections::BTreeSet;
use std::ffi::OsString;

use super::{Point, Prepared, ProjectError};

impl Prepared {
    pub fn verify_stage(&self) -> Result<(), ProjectError> {
        self.root.verify()?;
        let mut expected = BTreeSet::new();
        if let Some(manifest) = &self.manifest {
            manifest.verify()?;
            expected.insert(OsString::from("manifest.json"));
        }
        for (index, operation) in self.operations.iter().enumerate() {
            if let Some(candidate) = &operation.candidate {
                candidate.verify()?;
                expected.insert(OsString::from(format!("candidate-{index}")));
            }
            if let Some(backup) = &operation.backup {
                backup.verify()?;
                expected.insert(OsString::from(format!("backup-{index}")));
            }
        }
        let actual: BTreeSet<_> = self
            .root
            .entries(self.operations.len() * 2 + 2)?
            .into_iter()
            .collect();
        if actual != expected {
            return Err(changed());
        }
        Ok(())
    }

    pub fn verify_installed(&self) -> Result<(), ProjectError> {
        self.verify_stage()?;
        self.parents.verify()?;
        for operation in &self.operations {
            operation.installed.as_ref().ok_or_else(changed)?.verify()?;
        }
        Ok(())
    }

    pub fn cleanup(
        &mut self,
        hook: &mut impl FnMut(Point) -> Result<(), ProjectError>,
    ) -> Result<(), ProjectError> {
        self.verify_stage()?;
        let mut step = 0;
        for operation in &mut self.operations {
            if let Some(backup) = &operation.backup {
                hook(Point::BeforeCleanup(step))?;
                backup.remove()?;
                operation.backup = None;
                hook(Point::AfterCleanup(step))?;
                step += 1;
            }
            if let Some(candidate) = &operation.candidate {
                hook(Point::BeforeCleanup(step))?;
                candidate.remove()?;
                operation.candidate = None;
                hook(Point::AfterCleanup(step))?;
                step += 1;
            }
        }
        // Remove the manifest last so a failed cleanup retains the recovery inventory.
        self.verify_stage()?;
        if let Some(manifest) = &self.manifest {
            hook(Point::BeforeCleanup(step))?;
            manifest.remove()?;
            self.manifest = None;
            hook(Point::AfterCleanup(step))?;
            step += 1;
        }
        hook(Point::BeforeCleanup(step))?;
        self.root.remove_empty()?;
        // Once the transaction root is removed, no additional fallible cleanup step is needed.
        Ok(())
    }
}

fn changed() -> ProjectError {
    super::failure(
        crate::project::filesystem::TRANSACTION,
        "ownership",
        "transaction directory or recovery material changed",
    )
}
