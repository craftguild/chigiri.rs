//! Captures manifest bytes and schema roots once to prevent path-based rereads.

use std::path::Path;

use crate::CodegenError;
use crate::filesystem::{Directory, FsError};

use super::Manifest;

pub(crate) const MAX_MANIFEST_BYTES: usize = 64 * 1024;

/// The output authority that captured the manifest and input roots once.
#[derive(Clone)]
pub struct ResolvedManifest {
    pub(crate) manifest: Manifest,
    pub(crate) root: Directory,
    pub(crate) schema: Directory,
    pub(crate) label: String,
    pub(crate) bytes: Vec<u8>,
}

impl std::fmt::Debug for ResolvedManifest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedManifest")
            .finish_non_exhaustive()
    }
}

impl ResolvedManifest {
    /// Returns the digest of captured relative paths and SDL bytes after validation.
    pub fn schema_digest(&self) -> Result<chigiri_core::SchemaDigest, CodegenError> {
        let compiled = self.compile()?;
        Ok(crate::provenance::schema_content_digest(&compiled.sources))
    }

    pub(crate) fn compile(&self) -> Result<crate::compile::CompiledSchema, CodegenError> {
        self.verify_paths()?;
        let limits = chigiri_core::ExecutionLimits::default();
        let sources = crate::discovery::discover_captured(&self.schema, &limits)?;
        let compiled = crate::compile::compile_sources(sources, &limits)?;
        self.manifest.resolve_domain_enums(&compiled.schema)?;
        self.verify_paths()?;
        Ok(compiled)
    }

    /// Reads a bounded manifest and captures its SDL directory without following links.
    pub fn load(path: &Path) -> Result<Self, CodegenError> {
        crate::cancellation::check()?;
        let (root, label) = manifest_parent(path)?;
        let bytes = root
            .read(&label, MAX_MANIFEST_BYTES)
            .map_err(|error| named_failure(error, &label))?;
        let source = std::str::from_utf8(&bytes).map_err(|_| {
            CodegenError::new("SF4011", format!("The manifest must be UTF-8: {label}"))
        })?;
        let manifest = Manifest::parse(&label, source)?;
        if super::shape::overlaps(&label, manifest.output()) {
            return Err(failure(FsError::Path));
        }
        let schema = root
            .relative(manifest.schema_directory())
            .map_err(failure)?;
        root.verify().map_err(failure)?;
        Ok(Self {
            manifest,
            root,
            schema,
            label,
            bytes,
        })
    }

    /// Return the validated manifest configuration.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Returns the exact captured manifest bytes used for provenance.
    pub fn manifest_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Return the manifest label relative to its captured root.
    ///
    /// Absolute paths are not embedded in provenance.
    pub fn manifest_label(&self) -> &str {
        &self.label
    }

    /// Detects replacement of captured directories or manifest bytes.
    pub fn verify_paths(&self) -> Result<(), CodegenError> {
        self.root
            .verify()
            .and_then(|()| self.schema.verify())
            .map_err(failure)?;
        if self
            .root
            .read(&self.label, MAX_MANIFEST_BYTES)
            .map_err(failure)?
            != self.bytes
        {
            return Err(failure(FsError::Changed));
        }
        Ok(())
    }

    /// The location of the actual consumer manifest that Cargo is testing.
    pub(crate) fn consumer_manifest(&self) -> Result<Option<(Directory, String)>, FsError> {
        self.manifest
            .consumer()
            .map(|consumer| self.root.parent_of(&consumer.cargo_manifest))
            .transpose()
    }
}

pub(super) fn manifest_parent(path: &Path) -> Result<(Directory, String), CodegenError> {
    let text = path.to_str().ok_or_else(|| failure(FsError::Path))?;
    if text.ends_with('/') || text.contains('\\') {
        return Err(failure(FsError::Path));
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| failure(FsError::Path))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let root = Directory::open(parent).map_err(failure)?;
    Ok((root, name.to_owned()))
}

pub(super) fn failure(error: FsError) -> CodegenError {
    named_failure(error, "manifest")
}

fn named_failure(error: FsError, label: &str) -> CodegenError {
    let code = match error {
        FsError::Unsupported => "SF4022",
        FsError::Io => "SF4008",
        _ => "SF4011",
    };
    // Select file to transfer only basename to the common redaction, and not to produce the original ancestor path or OS.
    CodegenError::new(code, format!("{label} I/O: {error}"))
}
