//! Manages candidate, backup, and recovery archives under one parent-directory
//! lock.

mod install;

use crate::contract::{RenderedContract, marker};
use crate::filesystem::{CapturedTree, Directory, DirectoryLock, FsError, OwnedFile, TreeLimits};
use crate::{CodegenDiagnostic, CodegenError, DiagnosticReport, ResolvedManifest};

use super::Operation;

pub(super) const STAGE: &str = ".chigiri-transaction";

pub(super) struct Transaction {
    pub(super) parent: Directory,
    pub(super) name: String,
    pub(super) stage: Directory,
    pub(super) candidate: Option<Directory>,
    pub(super) previous: Option<(Directory, CapturedTree)>,
    pub(super) expected: Option<CapturedTree>,
    lock: DirectoryLock,
    state: OwnedFile,
    pub(super) committed: bool,
    pub(super) preserve: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Step {
    Backup,
    Install,
    Verify,
    Uninstall,
    Rollback,
    RecoveryWrite,
    CleanupEntry,
    RecoveryRemove,
    StageRemove,
}

impl Transaction {
    pub(super) fn begin(
        manifest: &ResolvedManifest,
        operation: Operation,
    ) -> Result<Self, CodegenError> {
        crate::cancellation::check()?;
        manifest.verify_paths()?;
        let output = crate::filesystem::PortableRelativePath::new(manifest.manifest().output())
            .map_err(io)?;
        if manifest
            .root
            .overlaps_destination(
                &manifest.root.descendant_destination(&output).map_err(io)?,
                &manifest.schema.destination().map_err(io)?,
            )
            .map_err(io)?
        {
            return Err(io(FsError::Path));
        }
        let (parent, name) = manifest
            .root
            .ensure_parent(manifest.manifest().output())
            .map_err(io)?;
        let lock = parent.lock().map_err(io)?;
        for reserved in [STAGE, super::recovery::NAME] {
            if name.eq_ignore_ascii_case(reserved) {
                return Err(io(FsError::Path));
            }
            match parent.absent(reserved) {
                Ok(()) => {}
                Err(FsError::Exists) => {
                    return Err(CodegenError::new(
                        "SF4024",
                        "A previous transaction or recovery archive remains in the output parent. Inspect `.chigiri-transaction` and `.chigiri-recovery`, restore the output, and retry.",
                    ));
                }
                Err(error) => return Err(io(error)),
            }
        }
        let previous = match operation {
            Operation::Generate => {
                parent.absent(&name).map_err(io)?;
                None
            }
            Operation::Update => {
                let directory = parent.relative(&name).map_err(io)?;
                let tree = capture(&directory)?;
                marker::read(&tree)?;
                Some((directory, tree))
            }
        };
        let stage = parent
            .create_directory(STAGE)
            .map_err(|error| retained(io(error), false))?;
        let state = serde_json::to_vec(&serde_json::json!({
            "schema_version": "chigiri/transaction-v1",
            "output": name,
            "operation": if operation == Operation::Generate { "generate" } else { "update" },
        }))
        .map_err(|_| io(FsError::Io))?;
        let state = stage
            .create_stream("state.json", 4096, |writer| writer.write(&state))
            .map_err(|error| retained(io(error), false))?;
        Ok(Self {
            parent,
            name,
            stage,
            candidate: None,
            previous,
            expected: None,
            lock,
            state,
            committed: false,
            preserve: false,
        })
    }

    pub(super) fn prepare(&mut self, rendered: RenderedContract) -> Result<(), CodegenError> {
        crate::cancellation::check()?;
        let expected = CapturedTree::from_files(rendered.files);
        self.expected = Some(expected);
        self.candidate = Some(self.stage.create_directory("candidate").map_err(io)?);
        self.candidate
            .as_ref()
            .unwrap()
            .write_tree_with(
                self.expected.as_ref().unwrap(),
                crate::cancellation::filesystem,
            )
            .map_err(io)?;
        self.verify_candidate()
    }

    pub(super) fn verify_candidate(&self) -> Result<(), CodegenError> {
        self.lock.verify().map_err(io)?;
        let tree = capture(
            self.candidate
                .as_ref()
                .ok_or_else(|| io(FsError::Missing))?,
        )?;
        if Some(&tree) != self.expected.as_ref() {
            return Err(io(FsError::Changed));
        }
        marker::read(&tree)?;
        Ok(())
    }

    pub(super) fn cleanup_stage(&self) -> Result<(), CodegenError> {
        let remaining = self
            .stage
            .capture(
                TreeLimits {
                    depth: 0,
                    entries: 1,
                    files: 1,
                    file_bytes: 4096,
                    total_bytes: 4096,
                },
                |_| true,
            )
            .map_err(io)?;
        if remaining.files.len() != 1 || !remaining.files.contains_key("state.json") {
            return Err(io(FsError::Changed));
        }
        self.state.remove().map_err(io)?;
        self.stage.remove_empty().map_err(io)
    }

    pub(super) fn finish_failure(&mut self, error: CodegenError) -> CodegenError {
        crate::cancellation::recovering(|| self.finish_recovery(error))
    }

    fn finish_recovery(&mut self, error: CodegenError) -> CodegenError {
        if self.committed || self.preserve {
            return retained(error, self.committed);
        }
        let cleanup = (|| {
            if let Some(candidate) = &self.candidate {
                let actual = capture(candidate)?;
                let expected = self.expected.as_ref().ok_or_else(|| io(FsError::Changed))?;
                // Delete only files recorded in the ownership inventory.
                if !actual.directories.is_subset(&expected.directories)
                    || actual
                        .files
                        .iter()
                        .any(|(path, bytes)| expected.files.get(path) != Some(bytes))
                {
                    return Err(io(FsError::Changed));
                }
                candidate.remove_tree(&actual, &mut || Ok(())).map_err(io)?;
            }
            self.cleanup_stage()
        })();
        match cleanup {
            Ok(()) => error,
            Err(cleanup) => retained(combine(error, cleanup, "Candidate cleanup failed."), false),
        }
    }
}

pub(super) fn capture(directory: &Directory) -> Result<CapturedTree, CodegenError> {
    directory
        .capture_limited(TreeLimits::generated(), |path| {
            Some(if path == crate::GENERATED_MARKER_FILE {
                1024 * 1024
            } else {
                64 * 1024 * 1024
            })
        })
        .map_err(io)
}

pub(super) fn io(error: FsError) -> CodegenError {
    let code = match error {
        FsError::Unsupported => "SF4022",
        FsError::Io => "SF4008",
        FsError::Busy => "SF4024",
        FsError::Cancelled => "SF4026",
        _ => "SF4020",
    };
    CodegenError::new(code, format!("generated crate transaction: {error}"))
}

pub(super) fn combine(primary: CodegenError, secondary: CodegenError, note: &str) -> CodegenError {
    let mut diagnostics = primary.report().diagnostics().to_vec();
    diagnostics.extend(secondary.report().diagnostics().iter().cloned());
    diagnostics.push(CodegenDiagnostic::error("SF4024", note));
    CodegenError::from_report(DiagnosticReport::new(diagnostics, false))
}

pub(super) fn retained(error: CodegenError, committed: bool) -> CodegenError {
    let mut diagnostics = error.report().diagnostics().to_vec();
    diagnostics.push(CodegenDiagnostic::error("SF4024", if committed {
        "The newly generated crate is already live."
    } else {
        "The transaction could not complete. Inspect the live output and retained transaction trees."
    }).with_note("`.chigiri-transaction/state.json` identifies the generated output parent: `previous` names the old tree and `candidate` names the new tree. `.chigiri-recovery` contains an archive of every file from the old tree. Keep these artifacts until recovery is complete."));
    CodegenError::from_report(DiagnosticReport::new(diagnostics, false))
}
