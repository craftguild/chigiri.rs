//! Build project contracts from adopted public SDL and confined local inputs.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use chigiri::core::Source;
use chigiri_host::PortableRelativePath;

use super::filesystem::{Directory, FileSnapshot};
use super::management::Management;
use super::normalize::normalized_sources;
use super::{Layout, ProjectError, ProjectSettings, evaluate_config};
use crate::ir::MockIr;

pub(super) struct CapturedInput {
    pub logical: PortableRelativePath,
    pub file: FileSnapshot,
}

/// A validated project schema snapshot that does not implicitly run resolver readiness checks.
pub struct ProjectSchema {
    pub(super) directory: Directory,
    pub(super) files: Vec<CapturedInput>,
    management_files: Vec<FileSnapshot>,
    settings: ProjectSettings,
    ir: Arc<MockIr>,
}

impl ProjectSchema {
    /// Return the evaluated project settings.
    pub fn settings(&self) -> &ProjectSettings {
        &self.settings
    }
    /// Return the IR built from the adopted public and current private SDL.
    pub fn ir(&self) -> &Arc<MockIr> {
        &self.ir
    }
    /// Verify the captured local contract and generation without reopening public locators.
    pub fn verify_unchanged(&self) -> Result<(), ProjectError> {
        self.verify_inputs_unchanged()?;
        for file in &self.management_files {
            file.verify()?;
        }
        self.directory.transaction_absent()
    }

    /// Verify authored inputs while a writer intentionally replaces generated management files.
    pub(super) fn verify_inputs_unchanged(&self) -> Result<(), ProjectError> {
        self.directory.verify()?;
        for input in &self.files {
            input.file.verify()?;
        }
        Ok(())
    }
}

/// Read the adopted project contract and verify generated ownership, but not modules or seeds.
pub fn read_project_schema(path: &Path) -> Result<ProjectSchema, ProjectError> {
    let directory = Directory::open(path)?;
    directory.transaction_absent()?;
    let (schema, management) = from_adopted(directory)?;
    current_layout(&schema, &management)?;
    schema.verify_unchanged()?;
    management.verify()?;
    Ok(schema)
}

pub(super) fn from_adopted(
    directory: Directory,
) -> Result<(ProjectSchema, Management), ProjectError> {
    let (settings, config) = read_config(&directory)?;
    let management = Management::read(&directory)?;
    let public = management.snapshot.public_schema.to_sources()?;
    let (storage, storage_files) = read_storage(&directory, &settings.storage_sources)?;
    raw_budget(
        public.iter().map(|source| source.text().len()).sum(),
        &storage_files,
    )?;
    let files = std::iter::once(config).chain(storage_files).collect();
    let schema = build(
        directory,
        files,
        management.files.clone(),
        settings,
        public,
        storage,
    )?;
    schema.verify_unchanged()?;
    management.verify()?;
    Ok((schema, management))
}

pub(super) fn from_external(
    directory: Directory,
) -> Result<(ProjectSchema, Management), ProjectError> {
    let (settings, config) = read_config(&directory)?;
    let management = Management::read(&directory)?;
    from_external_parts(directory, settings, config, management)
}

pub(super) fn from_external_parts(
    directory: Directory,
    settings: ProjectSettings,
    config: CapturedInput,
    management: Management,
) -> Result<(ProjectSchema, Management), ProjectError> {
    let (public, public_files) = read_sources(&directory, &settings.sources)?;
    let (storage, storage_files) = read_storage(&directory, &settings.storage_sources)?;
    raw_budget(0, &public_files)?;
    raw_budget(
        public_files
            .iter()
            .map(|input| input.file.bytes.len())
            .sum(),
        &storage_files,
    )?;
    reject_duplicate_roles(&public_files, &storage_files)?;
    let files = std::iter::once(config)
        .chain(public_files)
        .chain(storage_files)
        .collect();
    let schema = build(
        directory,
        files,
        management.files.clone(),
        settings,
        public,
        storage,
    )?;
    schema.verify_unchanged()?;
    management.verify()?;
    Ok((schema, management))
}

pub(super) fn current_layout(
    schema: &ProjectSchema,
    management: &Management,
) -> Result<Layout, ProjectError> {
    let layout = Layout::with_events(schema.ir(), &schema.settings().events)?;
    management.current(schema.ir(), &layout, schema.settings())?;
    let contract =
        crate::luau::Contract::with_state(schema.ir().clone(), schema.settings().state.clone())
            .map_err(|error| {
                let mut failure = ProjectError::new(error.phase, error.message);
                if let Some(position) = error.source_position() {
                    failure.set_position(position);
                }
                failure
            })?;
    if contract.declarations() != management.files[0].bytes {
        return Err(ownership("generated type declaration drift"));
    }
    Ok(layout)
}

pub(super) fn read_sources(
    directory: &Directory,
    paths: &[String],
) -> Result<(Vec<Source>, Vec<CapturedInput>), ProjectError> {
    if paths.is_empty() {
        return Err(ProjectError::new("schema", "SDL source count limit"));
    }
    let (raw, files) = read_role(directory, paths, "schema", false)?;
    Ok((normalized_sources("schema", &raw)?, files))
}

fn read_storage(
    directory: &Directory,
    paths: &[String],
) -> Result<(Vec<Source>, Vec<CapturedInput>), ProjectError> {
    let (raw, files) = read_role(directory, paths, "storage", true)?;
    Ok((normalized_sources("storage", &raw)?, files))
}

fn read_role(
    directory: &Directory,
    paths: &[String],
    role: &str,
    confined: bool,
) -> Result<(Vec<Source>, Vec<CapturedInput>), ProjectError> {
    if paths.len() > crate::limits::SDL_SOURCES {
        return Err(ProjectError::new("schema", "SDL source count limit"));
    }
    let mut identities = BTreeSet::new();
    let mut sources = Vec::new();
    let mut files = Vec::new();
    let mut remaining = crate::schema::MAX_SDL_BYTES;
    for (index, path) in paths.iter().enumerate() {
        let logical = super::logical(&format!("{role}/{:03}.graphql", index + 1))?;
        let path = if confined {
            PortableRelativePath::new(path)
                .map_err(|_| ProjectError::new("schema", "portable storage SDL paths required"))?
                .as_str()
                .to_owned()
        } else if super::config_decode::valid_source(path) {
            path.clone()
        } else {
            return Err(ProjectError::new("schema", "explicit SDL paths required"));
        };
        let file = directory
            .read(Path::new(&path), remaining)
            .map_err(|mut error| {
                error.set_file(logical.clone());
                error
            })?;
        if !identities.insert(file.identity()) {
            return Err(ProjectError::new("schema", "duplicate SDL source"));
        }
        remaining -= file.bytes.len();
        sources.push(Source::new(logical.as_str(), file.bytes.clone()));
        files.push(CapturedInput { logical, file });
    }
    Ok((sources, files))
}

pub(super) fn read_config(
    directory: &Directory,
) -> Result<(ProjectSettings, CapturedInput), ProjectError> {
    let file = read_config_file(directory)?;
    let settings = evaluate_config(&file.file.bytes).map_err(|mut error| {
        error.set_file("mock.luau");
        error
    })?;
    Ok((settings, file))
}

pub(super) fn read_config_file(directory: &Directory) -> Result<CapturedInput, ProjectError> {
    let file = directory
        .read(Path::new("mock.luau"), crate::limits::CONFIG_BYTES)
        .map_err(|mut error| {
            error.set_file("mock.luau");
            error
        })?;
    Ok(CapturedInput {
        logical: super::fixed_path("mock.luau"),
        file,
    })
}

fn build(
    directory: Directory,
    files: Vec<CapturedInput>,
    management_files: Vec<FileSnapshot>,
    settings: ProjectSettings,
    public: Vec<Source>,
    storage: Vec<Source>,
) -> Result<ProjectSchema, ProjectError> {
    let public_count = public.len();
    let ir = MockIr::parse_with_storage(public, storage).map_err(|error| {
        let mut failure = ProjectError::new("schema", error.code);
        if let Some(span) = error.span {
            let source = span.source_id.index();
            failure.set_file(if source < public_count {
                format!("schema/{:03}.graphql", source + 1)
            } else {
                format!("storage/{:03}.graphql", source - public_count + 1)
            });
        }
        failure
    })?;
    super::config_events::validate(&settings.events, &ir)?;
    Ok(ProjectSchema {
        directory,
        files,
        management_files,
        settings,
        ir: Arc::new(ir),
    })
}

fn reject_duplicate_roles(
    public: &[CapturedInput],
    storage: &[CapturedInput],
) -> Result<(), ProjectError> {
    let identities: BTreeSet<_> = public.iter().map(|input| input.file.identity()).collect();
    if storage
        .iter()
        .any(|input| identities.contains(&input.file.identity()))
    {
        return Err(ProjectError::new("schema", "duplicate SDL source"));
    }
    Ok(())
}

pub(super) fn raw_budget(mut bytes: usize, inputs: &[CapturedInput]) -> Result<(), ProjectError> {
    for input in inputs {
        bytes = bytes
            .checked_add(input.file.bytes.len())
            .ok_or_else(|| ProjectError::new("limit", "SDL byte budget exceeded"))?;
        if bytes > crate::schema::MAX_SDL_BYTES {
            let mut error = ProjectError::new("limit", "SDL byte budget exceeded");
            error.set_file(input.logical.clone());
            return Err(error);
        }
    }
    Ok(())
}

fn ownership(message: &'static str) -> ProjectError {
    let mut error = ProjectError::new("ownership", message);
    error.set_file("generated/update-state.json");
    error
}
