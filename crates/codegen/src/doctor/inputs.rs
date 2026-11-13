//! Validate that doctor manifest/lock inputs match Cargo's cache layout.

use std::path::Path;
use std::process::Command;

use crate::filesystem::{Directory, FsError};
use crate::{
    CodegenDiagnostic, CodegenError, ContractCheck, DiagnosticLabel, DiagnosticReport,
    ResolvedManifest,
};

use super::metadata::{Metadata, Package, contract, parent};

const MAX_CARGO_FILE: usize = 1024 * 1024;

pub(super) struct Inputs<'a> {
    pub(super) manifest: &'a ResolvedManifest,
    pub(super) checked: ContractCheck,
    pub(super) requested: File,
    pub(super) selected: Option<File>,
    pub(super) workspace_manifest: Option<File>,
    pub(super) lock: Option<File>,
}

pub(super) struct File {
    pub(super) directory: Directory,
    pub(super) name: String,
    bytes: Vec<u8>,
}

impl File {
    pub(super) fn read(root: &Directory, path: &Path) -> Result<Self, CodegenError> {
        let directory = root.resolve_directory(parent(path)?).map_err(io)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io(FsError::Path))?;
        Self::read_child(root, directory, name)
    }

    fn read_child(
        root: &Directory,
        directory: Directory,
        name: &str,
    ) -> Result<Self, CodegenError> {
        if !root.contains_directory(&directory).map_err(io)? {
            return Err(contract(
                "The consumer Cargo manifest must be inside the Chigiri manifest root.",
            ));
        }
        let name = name.to_owned();
        let bytes = directory.read(&name, MAX_CARGO_FILE).map_err(io)?;
        Ok(Self {
            directory,
            name,
            bytes,
        })
    }

    pub(super) fn push_argument<'a>(
        &self,
        command: &'a mut Command,
    ) -> Result<&'a mut Command, CodegenError> {
        let path = crate::filesystem::PortableRelativePath::new(&self.name).map_err(io)?;
        self.directory
            .push_descendant_argument(command, &path)
            .map_err(io)
    }

    pub(super) fn verify(&self) -> Result<(), CodegenError> {
        if self
            .directory
            .read(&self.name, MAX_CARGO_FILE)
            .map_err(io)?
            != self.bytes
        {
            return Err(io(FsError::Changed));
        }
        Ok(())
    }

    fn toml(&self, root: &Directory) -> Result<(), CodegenError> {
        let source = std::str::from_utf8(&self.bytes).map_err(|_| io(FsError::Path))?;
        source
            .parse::<toml_edit::ImDocument<String>>()
            .map_err(|error| {
                let start = error.span().map_or(0, |span| span.start).min(source.len());
                let before = source.get(..start).unwrap_or_default();
                let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
                let column = before
                    .rsplit('\n')
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .count()
                    + 1;
                let label = root
                    .descendant_label(&self.directory, &self.name)
                    .map_or_else(|_| "<path>".to_owned(), |path| path.to_string());
                CodegenError::from_report(DiagnosticReport::new(
                    vec![
                        CodegenDiagnostic::error(
                            "SF4015",
                            "The consumer cargo TOML syntax is false.",
                        )
                        .with_primary(DiagnosticLabel::new(
                            &label,
                            start..start,
                            line,
                            column,
                            "Cargo manifest",
                        )),
                    ],
                    false,
                ))
            })?;
        Ok(())
    }
}

impl<'a> Inputs<'a> {
    pub(super) fn new(
        manifest: &'a ResolvedManifest,
        checked: ContractCheck,
    ) -> Result<Self, CodegenError> {
        let (directory, name) = manifest
            .consumer_manifest()
            .map_err(io)?
            .ok_or_else(|| contract("The doctor needs a consumer manifest."))?;
        let requested = File::read_child(&manifest.root, directory, &name)?;
        requested.toml(&manifest.root)?;
        Ok(Self {
            manifest,
            checked,
            requested,
            selected: None,
            workspace_manifest: None,
            lock: None,
        })
    }

    pub(super) fn select(
        &mut self,
        metadata: &Metadata,
        selected: &Package,
    ) -> Result<(), CodegenError> {
        let root = &self.manifest.root;
        let file = File::read(root, &selected.manifest_path)?;
        file.toml(root)?;
        let workspace = File::read(root, &metadata.workspace_root.join("Cargo.toml"))?;
        workspace.toml(root)?;
        let lock = File::read(root, &metadata.workspace_root.join("Cargo.lock")).map_err(|_| {
            contract("Create or update the consumer Cargo.lock with Cargo, then run doctor again.")
        })?;
        self.selected = Some(file);
        self.workspace_manifest = Some(workspace);
        self.lock = Some(lock);
        self.cache(metadata, selected)?;
        self.verify()
    }

    pub(super) fn cache(
        &self,
        metadata: &Metadata,
        selected: &Package,
    ) -> Result<(), CodegenError> {
        let root = &self.manifest.root;
        let output =
            crate::filesystem::PortableRelativePath::new(self.manifest.manifest().output())
                .map_err(io)?;
        let mut forbidden = vec![
            root.descendant_destination(&output).map_err(io)?,
            self.manifest.schema.destination().map_err(io)?,
        ];
        for target in &selected.targets {
            if !target.src_path.is_absolute() {
                return Err(super::metadata::invalid());
            }
            forbidden.push(
                root.resolve_destination(parent(&target.src_path)?)
                    .map_err(io)?,
            );
        }
        for path in
            std::iter::once(&metadata.target_directory).chain(metadata.build_directory.iter())
        {
            let resolved = root.resolve_destination(path).map_err(io)?;
            for input in &forbidden {
                if root.overlaps_destination(&resolved, input).map_err(io)? {
                    return Err(CodegenError::new(
                        "SF4023",
                        "The consumer cargo cache/build layout overlaps the SDK, SDL, or consumer source.",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn verify(&self) -> Result<(), CodegenError> {
        crate::cancellation::check()?;
        self.checked.verify(self.manifest)?;
        self.requested.verify()?;
        for file in [&self.selected, &self.workspace_manifest, &self.lock]
            .into_iter()
            .flatten()
        {
            file.verify()?;
        }
        Ok(())
    }
}

pub(super) fn io(error: FsError) -> CodegenError {
    let code = match error {
        FsError::Unsupported => "SF4022",
        FsError::Io => "SF4008",
        FsError::Cancelled => "SF4026",
        _ => "SF4015",
    };
    CodegenError::new(code, format!("consumer I/O: {error}"))
}
