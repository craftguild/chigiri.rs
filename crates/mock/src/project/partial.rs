//! Report adoption-set readiness separately from required conformance and retained-file checks.

use std::collections::{BTreeMap, BTreeSet};

use super::{ActiveKind, ActiveSnapshot, ProjectError};
use crate::checked::CheckError;
use crate::ir::ModelId;
use crate::luau::{HookCandidate, ScriptCandidate};
use crate::store::SeedData;

/// Result of targeted validation followed by normal whole-project readiness evaluation.
pub enum Readiness {
    /// All modules, seeds, and bundle capacity satisfy normal startup conditions.
    Ready,
    /// Unrelated retained files prevent whole-project readiness.
    NotReady(ProjectError),
    /// Whole-project readiness could not be evaluated because of an environment failure.
    NotEvaluated(ProjectError),
}

impl super::validation::ValidationView<'_> {
    /// Validate a closed replacement set, then recheck it with the latest retained project files.
    /// The project lock is not held during long checks, so the caller must verify the snapshot separately.
    pub fn validate_selection(
        &self,
        replacements: &[(String, String)],
    ) -> Result<Readiness, ProjectError> {
        self.validate_selection_with(replacements, || self.check_replacements(replacements))
    }

    /// Test seam that injects environment failures only into the whole-project check.
    pub(super) fn validate_selection_with(
        &self,
        replacements: &[(String, String)],
        whole_check: impl FnOnce() -> Result<crate::checked::CheckedMock, ProjectError>,
    ) -> Result<Readiness, ProjectError> {
        let sources = self.replacement_sources(replacements)?;
        self.check_capacity(&sources)?;
        self.validate_sources(&sources, &sources.keys().copied().collect(), None)?;
        match whole_check() {
            Ok(_) => Ok(Readiness::Ready),
            // A target check cannot turn a whole-project resource failure into success.
            Err(error)
                if matches!(
                    error.phase,
                    "limit" | "memory" | "budget" | "timeout" | "cancelled"
                ) =>
            {
                Err(error)
            }
            Err(error) if error.exit == 2 => Ok(Readiness::NotEvaluated(error)),
            Err(error) => Ok(Readiness::NotReady(error)),
        }
    }

    pub(super) fn validate_sources(
        &self,
        sources: &BTreeMap<&str, &str>,
        selected: &BTreeSet<&str>,
        control: Option<&crate::validation_control::ValidationControl>,
    ) -> Result<(), ProjectError> {
        let mut scripts = Vec::new();
        let mut hooks = Vec::new();
        let mut consumers = Vec::new();
        let mut models = BTreeSet::new();
        let mut seed_bytes = 0usize;
        for file in self.files() {
            let source = sources.get(file.path()).copied().unwrap_or(file.source());
            match file.kind() {
                ActiveKind::Resolver(script) => {
                    if source.len() > crate::luau::MAX_BODY {
                        return Err(limit(file.path()));
                    }
                    if selected.contains(file.path()) {
                        scripts.push(ScriptCandidate {
                            script,
                            source: source.to_owned(),
                        });
                    }
                }
                ActiveKind::Seed(model) => {
                    if source.len() > crate::limits::SEED_BYTES.saturating_sub(seed_bytes) {
                        return Err(limit(file.path()));
                    }
                    seed_bytes += source.len();
                    if selected.contains(file.path()) {
                        models.insert(model);
                    }
                }
                ActiveKind::Hook(slot) => {
                    if selected.contains(file.path()) {
                        hooks.push(HookCandidate {
                            slot,
                            source: source.to_owned(),
                        });
                    }
                }
                ActiveKind::Consumer(index) => {
                    if selected.contains(file.path()) {
                        consumers.push(crate::luau::ConsumerCandidate {
                            registration: self.layout.consumers[index].clone(),
                            source: source.to_owned(),
                        });
                    }
                }
            }
        }
        let error = |error| super::read::check_error(self.ir, self.layout, error);
        // Use the same validation-thread stack size as a full project check.
        checkpoint(control)?;
        let checked = std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("chigiri-mock-partial".into())
                .stack_size(8 * 1024 * 1024)
                .spawn_scoped(scope, || {
                    self.contract()
                        .check_selected_modules(scripts, hooks, consumers, control)
                })
                .map_err(|_| ProjectError::new("environment", "cannot create validation worker"))?
                .join()
                .map_err(|_| ProjectError::new("environment", "validation worker failed"))?
                .map_err(|e| error(CheckError::Script(e)))
        });
        checkpoint(control)?;
        checked?;
        if !models.is_empty() {
            let models = reference_closed_models(self.ir, self.files(), sources, models)
                .map_err(|e| error(CheckError::Seed(e)))?;
            let seeds: Vec<_> = self
                .files()
                .iter()
                .filter_map(|file| match file.kind() {
                    ActiveKind::Seed(model) if models.contains(&model) => Some((
                        model,
                        sources.get(file.path()).copied().unwrap_or(file.source()),
                    )),
                    _ => None,
                })
                .collect();
            let checked = SeedData::check_component(self.ir.clone(), &seeds)
                .map_err(|e| error(CheckError::Seed(e)));
            checkpoint(control)?;
            checked?;
        }
        Ok(())
    }

    pub(super) fn replacement_sources<'a>(
        &self,
        replacements: &'a [(String, String)],
    ) -> Result<BTreeMap<&'a str, &'a str>, ProjectError> {
        if replacements.len() > self.files().len() {
            return Err(ProjectError::new(
                "ownership",
                "invalid replacement inventory",
            ));
        }
        let mut sources = BTreeMap::new();
        for (path, source) in replacements {
            if !self.files().iter().any(|file| file.path() == path)
                || sources.insert(path.as_str(), source.as_str()).is_some()
            {
                return Err(ProjectError::new(
                    "ownership",
                    "unknown or duplicate replacement target",
                ));
            }
        }
        Ok(sources)
    }
}

fn reference_closed_models(
    ir: &crate::ir::MockIr,
    files: &[super::validation::SourceView<'_>],
    sources: &BTreeMap<&str, &str>,
    mut models: BTreeSet<ModelId>,
) -> Result<BTreeSet<ModelId>, crate::store::SeedError> {
    loop {
        models = connected_models(ir, models);
        let before = models.len();
        for file in files {
            let ActiveKind::Seed(model) = file.kind() else {
                continue;
            };
            if models.contains(&model) {
                let source = sources.get(file.path()).copied().unwrap_or(file.source());
                models.extend(SeedData::reference_dependencies(ir, model, source)?);
            }
        }
        if models.len() == before {
            return Ok(models);
        }
    }
}

/// Expand a model set to the owner and target of every connected relation.
pub(super) fn connected_models(
    ir: &crate::ir::MockIr,
    mut models: BTreeSet<ModelId>,
) -> BTreeSet<ModelId> {
    loop {
        let before = models.len();
        for relation in ir.relations() {
            if models.contains(&relation.owner) || models.contains(&relation.target) {
                models.insert(relation.owner);
                models.insert(relation.target);
            }
        }
        if models.len() == before {
            return models;
        }
    }
}

fn limit(path: &str) -> ProjectError {
    let mut error = ProjectError::new("limit", "active source byte limit");
    error.set_file(path);
    error
}

fn checkpoint(
    control: Option<&crate::validation_control::ValidationControl>,
) -> Result<(), ProjectError> {
    control.map_or(Ok(()), |control| control.project())
}

impl ActiveSnapshot {
    /// Validate replacements and retained files through the normal readiness machinery.
    pub fn validate_selection(
        &self,
        replacements: &[(String, String)],
    ) -> Result<Readiness, ProjectError> {
        self.validation()?.validate_selection(replacements)
    }
}
