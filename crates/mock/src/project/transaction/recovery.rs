//! Persist the primary cause, bounded recovery failures, and each object's remaining location in a new record.

use std::ffi::OsStr;

use serde::Serialize;

use super::{Digest, Prepared, ProjectError, metadata};

#[derive(Serialize)]
struct Record<'a> {
    format: &'static str,
    committed: bool,
    old_generation: &'a Digest,
    new_generation: &'a Digest,
    primary: Failure<'a>,
    recovery: Vec<Failure<'a>>,
    operations: Vec<Operation<'a>>,
}

#[derive(Serialize)]
struct Failure<'a> {
    file: &'a str,
    phase: &'static str,
    message: &'static str,
}

impl<'a> From<&'a ProjectError> for Failure<'a> {
    fn from(error: &'a ProjectError) -> Self {
        Self {
            file: &error.file,
            phase: error.phase,
            message: error.message,
        }
    }
}

#[derive(Serialize)]
struct Operation<'a> {
    path: &'a str,
    original: Option<Digest>,
    next: Option<Digest>,
    candidate: Option<String>,
    backup: Option<String>,
    installed: bool,
    restored: bool,
}

impl Prepared {
    pub fn record_failure(&self, error: &mut ProjectError) {
        let result = (|| {
            let record = Record {
                format: "chigiri-mock-recovery/v1",
                committed: error.committed,
                old_generation: &self.old_generation,
                new_generation: &self.new_generation,
                primary: Failure::from(&*error),
                recovery: error.recovery.iter().map(Failure::from).collect(),
                operations: self
                    .operations
                    .iter()
                    .enumerate()
                    .map(|(index, operation)| {
                        let next = operation
                            .candidate
                            .as_ref()
                            .or(operation.installed.as_ref());
                        Operation {
                            path: &operation.path,
                            original: operation
                                .original
                                .as_ref()
                                .map(|file| Digest::bytes(&file.bytes)),
                            next: next.map(|file| Digest::bytes(file.bytes.as_bytes())),
                            candidate: operation
                                .candidate
                                .as_ref()
                                .map(|_| format!("candidate-{index}")),
                            backup: operation.backup.as_ref().map(|_| format!("backup-{index}")),
                            installed: operation.installed.is_some(),
                            restored: operation.restored.is_some(),
                        }
                    })
                    .collect(),
            };
            let bytes = metadata::render(&record, self.recovery_budget)?;
            self.root.create_file(OsStr::new("recovery.json"), &bytes)?;
            Ok::<_, ProjectError>(())
        })();
        if let Err(mut failure) = result {
            failure.set_file(format!(
                "{}/recovery.json",
                crate::project::filesystem::TRANSACTION
            ));
            error.recovery.push(failure);
        }
    }
}
