//! Record each backup and candidate rename immediately so every affected path can be restored after failure.

use std::ffi::OsStr;

use super::{Point, Prepared, ProjectError};

pub(super) fn apply(
    prepared: &mut Prepared,
    hook: &mut impl FnMut(Point) -> Result<(), ProjectError>,
) -> Result<(), ProjectError> {
    for (index, operation) in prepared.operations.iter_mut().enumerate() {
        let parent = prepared.parents.obtain(&operation.parent)?;
        if let Some(original) = &operation.original {
            hook(Point::BeforeBackup(index))?;
            let result = original.move_to(&prepared.root, OsStr::new(&format!("backup-{index}")));
            match result {
                Ok(backup) => operation.backup = Some(backup),
                Err(failure) => {
                    operation.backup = failure.moved.map(|file| *file);
                    let mut error = *failure.error;
                    error.set_file(operation.path.clone());
                    return Err(error);
                }
            }
            hook(Point::AfterBackup(index))?;
        }
        hook(Point::BeforeInstall(index))?;
        let candidate = operation.candidate.take().ok_or_else(invalid)?;
        match candidate.move_to(&parent, &operation.name) {
            Ok(installed) => operation.installed = Some(installed),
            Err(failure) => {
                if let Some(moved) = failure.moved {
                    operation.installed = Some(*moved);
                } else {
                    operation.candidate = Some(candidate);
                }
                let mut error = *failure.error;
                error.set_file(operation.path.clone());
                return Err(error);
            }
        }
        hook(Point::AfterInstall(index))?;
    }
    Ok(())
}

pub(super) fn rollback(
    prepared: &mut Prepared,
    hook: &mut impl FnMut(Point) -> Result<(), ProjectError>,
    errors: &mut Vec<ProjectError>,
) {
    for (index, operation) in prepared.operations.iter_mut().enumerate().rev() {
        if operation.installed.is_none() && operation.backup.is_none() {
            continue;
        }
        if let Err(mut error) = hook(Point::BeforeRestore(index)) {
            error.set_file(operation.path.clone());
            errors.push(error);
            continue;
        }
        if let Some(installed) = operation.installed.take() {
            match installed.move_to(&prepared.root, OsStr::new(&format!("candidate-{index}"))) {
                Ok(candidate) => operation.candidate = Some(candidate),
                Err(failure) => {
                    if let Some(moved) = failure.moved {
                        operation.candidate = Some(*moved);
                    } else {
                        operation.installed = Some(installed);
                    }
                    let mut error = *failure.error;
                    error.set_file(operation.path.clone());
                    errors.push(error);
                }
            }
        }
        if let Some(backup) = operation.backup.take() {
            let result = prepared
                .parents
                .obtain(&operation.parent)
                .map(|parent| backup.move_to(&parent, &operation.name));
            match result {
                Ok(Ok(restored)) => operation.restored = Some(restored),
                Ok(Err(failure)) => {
                    if let Some(moved) = failure.moved {
                        operation.restored = Some(*moved);
                    } else {
                        operation.backup = Some(backup);
                    }
                    let mut error = *failure.error;
                    error.set_file(operation.path.clone());
                    errors.push(error);
                }
                Err(mut error) => {
                    operation.backup = Some(backup);
                    error.set_file(operation.path.clone());
                    errors.push(error);
                }
            }
        }
        if let Err(mut error) = hook(Point::AfterRestore(index)) {
            error.set_file(operation.path.clone());
            errors.push(error);
        }
    }
    // Restoration is not successful unless every restored snapshot still verifies.
    for operation in &prepared.operations {
        if let Some(restored) = &operation.restored
            && let Err(mut error) = restored.verify()
        {
            error.set_file(operation.path.clone());
            errors.push(error);
        }
    }
}

fn invalid() -> ProjectError {
    ProjectError::new("ownership", "transaction candidate missing")
}
