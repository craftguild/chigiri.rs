//! Capture ready resolver, type, hook, and seed inputs independently of later filesystem changes.

use std::path::Path;
use std::sync::Arc;

use super::filesystem::{Directory, FileSnapshot};
use super::management::{Management, read_file};
use super::{Layout, ProjectError, ProjectSchema};
use crate::checked::{CheckError, CheckedMock};
use crate::ir::{ModelId, ScriptId};
use crate::luau::Contract;
use chigiri_host::PortableRelativePath;

/// Active-file role resolved from IR rather than inferred from paths or contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveKind {
    /// Resolver required by the IR.
    Resolver(ScriptId),
    /// Seed file required by a storage model.
    Seed(ModelId),
    /// Enabled fixed hook slot.
    Hook(crate::hooks::HookSlot),
    /// Registered consumer position in the closed layout, including disabled modules.
    Consumer(usize),
}

/// One active file owned by the current IR and management snapshot.
pub struct ActiveFile {
    path: PortableRelativePath,
    coordinate: String,
    kind: ActiveKind,
    pub(super) snapshot: Arc<FileSnapshot>,
}

impl ActiveFile {
    /// Full project-relative path.
    pub fn path(&self) -> &str {
        self.path.as_str()
    }
    /// Canonical GraphQL coordinate, model name, or hook slot.
    pub fn coordinate(&self) -> &str {
        &self.coordinate
    }
    /// Return this file's IR role.
    pub fn kind(&self) -> ActiveKind {
        self.kind
    }
    /// Return UTF-8 captured from the original file descriptor.
    pub fn source(&self) -> &str {
        &self.snapshot.bytes
    }
}

/// Owned project snapshot for validation and serving.
/// Long-running processing uses captured values and does not retain the project lock.
pub struct ActiveSnapshot {
    pub(super) schema: ProjectSchema,
    pub(super) management: Management,
    pub(super) layout: Layout,
    files: Vec<ActiveFile>,
    contract: Contract,
    hooks: Vec<Arc<FileSnapshot>>,
}

impl ActiveSnapshot {
    /// Capture and validate every active resolver, hook, and seed input.
    pub fn capture(path: &Path) -> Result<Self, ProjectError> {
        let directory = Directory::open(path)?;
        let lock = directory.lock(false)?;
        let snapshot = Self::from_directory(directory)?;
        snapshot.check_capacity(&std::collections::BTreeMap::new())?;
        snapshot.verify_unchanged()?;
        lock.verify()?;
        Ok(snapshot)
    }

    pub(super) fn from_directory(directory: Directory) -> Result<Self, ProjectError> {
        directory.transaction_absent()?;
        let (schema, management) = super::sources::from_adopted(directory)?;
        let layout = super::sources::current_layout(&schema, &management)?;
        let contract = Contract::with_state(schema.ir().clone(), schema.settings().state.clone())
            .map_err(|error| {
            super::read::check_error(schema.ir(), &layout, CheckError::Script(error))
        })?;
        let hooks = read_hooks(&schema)?;
        let files = read_active(&schema, &layout, &hooks)?;
        Ok(Self {
            schema,
            management,
            layout,
            files,
            contract,
            hooks,
        })
    }

    /// Refresh active files under a short lock without reevaluating configuration.
    pub fn refreshed(mut self) -> Result<Self, ProjectError> {
        let lock = self.schema.directory.lock(false)?;
        self.schema.verify_unchanged()?;
        self.management.verify()?;
        self.schema.directory.transaction_absent()?;
        self.hooks = read_hooks(&self.schema)?;
        self.files = read_active(&self.schema, &self.layout, &self.hooks)?;
        self.check_capacity(&std::collections::BTreeMap::new())?;
        self.verify_unchanged()?;
        lock.verify()?;
        Ok(self)
    }

    /// Return the complete active-file inventory; callers track selected replacements separately.
    pub fn files(&self) -> &[ActiveFile] {
        &self.files
    }
    /// Return the captured schema and once-evaluated project settings.
    pub fn schema(&self) -> &ProjectSchema {
        &self.schema
    }
    /// Return the validation contract derived from the authoritative IR and host API.
    pub fn contract(&self) -> &Contract {
        &self.contract
    }

    /// Iterate all owned resolver, seed, and hook files for capacity and placement checks.
    pub(super) fn owned_files(&self) -> impl Iterator<Item = (&str, &FileSnapshot)> {
        self.files
            .iter()
            .filter(|file| !matches!(file.kind, ActiveKind::Hook(_)))
            .map(|file| (file.path(), file.snapshot.as_ref()))
            .chain(
                crate::hooks::HookSlot::ALL
                    .into_iter()
                    .zip(&self.hooks)
                    .map(|(slot, file)| (slot.path(), file.as_ref())),
            )
    }

    pub(crate) fn owned_sources(&self) -> impl Iterator<Item = (&str, &str)> {
        self.owned_files()
            .map(|(path, file)| (path, file.bytes.as_str()))
    }

    /// Reject replacement when any captured file or project input changed after the initial read.
    pub fn verify_unchanged(&self) -> Result<(), ProjectError> {
        self.schema.verify_unchanged()?;
        self.management.verify()?;
        for file in &self.files {
            file.snapshot.verify()?;
        }
        for file in &self.hooks {
            file.verify()?;
        }
        self.schema.directory.transaction_absent()
    }

    /// Evaluate complete readiness from the captured inputs without listening or resolving requests.
    pub fn check(&self) -> Result<CheckedMock, ProjectError> {
        self.check_replacements(&[])
    }

    pub(super) fn check_replacements(
        &self,
        replacements: &[(String, String)],
    ) -> Result<CheckedMock, ProjectError> {
        self.validation()?.check_replacements(replacements)
    }
}

fn read_hooks(schema: &ProjectSchema) -> Result<Vec<Arc<FileSnapshot>>, ProjectError> {
    crate::hooks::HookSlot::ALL
        .into_iter()
        .map(|slot| read_file(&schema.directory, slot.path(), crate::luau::MAX_BODY).map(Arc::new))
        .collect()
}

fn read_active(
    schema: &ProjectSchema,
    layout: &Layout,
    hooks: &[Arc<FileSnapshot>],
) -> Result<Vec<ActiveFile>, ProjectError> {
    let mut files = Vec::new();
    for entry in &layout.resolvers {
        files.push(ActiveFile {
            path: entry.path.clone(),
            coordinate: entry.coordinate.clone(),
            kind: ActiveKind::Resolver(entry.script),
            snapshot: Arc::new(read_file(
                &schema.directory,
                &entry.path,
                crate::luau::MAX_BODY,
            )?),
        });
    }
    for (index, consumer) in layout.consumers.iter().enumerate() {
        files.push(ActiveFile {
            path: consumer.path().clone(),
            coordinate: consumer.name().to_owned(),
            kind: ActiveKind::Consumer(index),
            snapshot: Arc::new(read_file(
                &schema.directory,
                consumer.path(),
                crate::luau::MAX_BODY,
            )?),
        });
    }
    let mut remaining = crate::limits::SEED_BYTES;
    for entry in &layout.seeds {
        let snapshot = read_file(&schema.directory, &entry.path, remaining)?;
        remaining -= snapshot.bytes.len();
        let coordinate = schema.ir().graph().types()[schema.ir().models()[entry.model.0].ty.0]
            .name
            .to_string();
        files.push(ActiveFile {
            path: entry.path.clone(),
            coordinate,
            kind: ActiveKind::Seed(entry.model),
            snapshot: Arc::new(snapshot),
        });
    }
    for (slot, snapshot) in crate::hooks::HookSlot::ALL.into_iter().zip(hooks) {
        if schema.settings().hooks.enabled(slot) {
            files.push(ActiveFile {
                path: super::fixed_path(slot.path()),
                coordinate: slot.name().into(),
                kind: ActiveKind::Hook(slot),
                snapshot: snapshot.clone(),
            });
        }
    }
    Ok(files)
}
