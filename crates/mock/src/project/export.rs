//! Write the canonical bundle from a checked snapshot to a new file outside the project.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::filesystem::{Directory, FileSnapshot};
use super::{ProjectError, read_project};

/// Export current initial seeds and active modules, excluding update history and live store state.
pub fn export_project(project: &Path, output: &Path) -> Result<(), ProjectError> {
    export_with(project, output, || {})
}

pub(super) fn export_with(
    project: &Path,
    output: &Path,
    before_publish: impl FnOnce(),
) -> Result<(), ProjectError> {
    let root = Directory::open(project)?;
    let parent = Directory::output_parent(output.parent().ok_or_else(destination)?)?;
    let name = output.file_name().ok_or_else(destination)?;
    if parent.is_within(&root)? {
        return Err(destination());
    }
    parent.absent(name)?;
    let checked = read_project(project)?;
    root.verify()?;
    let temporary = temporary(&parent, checked.bundle())?;
    let result = (|| {
        before_publish();
        root.verify()?;
        parent.verify()?;
        if parent.is_within(&root)? {
            return Err(destination());
        }
        temporary.move_to(&parent, name).map_err(|failure| {
            let mut error = *failure.error;
            error.committed = failure.moved.is_some();
            error
        })?;
        Ok(())
    })();
    if let Err(mut error) = result {
        if !error.committed
            && let Err(failure) = temporary.remove()
        {
            error.recovery.push(failure);
        }
        return Err(error);
    }
    Ok(())
}

fn temporary(parent: &Directory, bytes: &str) -> Result<FileSnapshot, ProjectError> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    for _ in 0..32 {
        let name = format!(
            ".chigiri-mock-export-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        match parent.create_file(OsStr::new(&name), bytes) {
            Ok(file) => return Ok(file),
            Err(error) if error.message == "exclusive destination already exists" => {}
            Err(mut error) => {
                error.set_file(name);
                return Err(error);
            }
        }
    }
    Err(ProjectError::new(
        "ownership",
        "cannot reserve private export file",
    ))
}

fn destination() -> ProjectError {
    ProjectError::new(
        "export",
        "new file outside project in an existing directory required",
    )
}
