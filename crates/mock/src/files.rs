//! Read bundle or explicit SDL through the project descriptor path and retain logical names for diagnostics.

use std::path::{Path, PathBuf};

use crate::diagnostic::Diagnostic;
use chigiri_mock::project::{InputFile, ProjectError};

pub(crate) struct Input {
    file: InputFile,
    path: PathBuf,
}

pub(crate) fn read(path: &Path, limit: usize) -> Result<Input, Diagnostic> {
    let file = InputFile::read(path, limit).map_err(|error| diagnostic(path, error))?;
    Ok(Input {
        file,
        path: path.to_owned(),
    })
}

impl Input {
    pub fn source(&self) -> &str {
        self.file.source()
    }
    pub fn identity(&self) -> chigiri_host::filesystem::FileIdentity {
        self.file.identity()
    }
    pub fn verify(&self) -> Result<(), Diagnostic> {
        self.file
            .verify()
            .map_err(|error| diagnostic(&self.path, error))
    }
}

fn diagnostic(path: &Path, error: ProjectError) -> Diagnostic {
    let mut result = Diagnostic::new(path, error.phase, error.message);
    result.exit = if error.message == "required project input missing" {
        2
    } else {
        error.exit
    };
    result
}
