//! Separates the borrowed validation view from its owned filesystem snapshot so
//! isolated workers can use the same validator.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::{ActiveKind, ActiveSnapshot, Layout, ProjectError, ServerSettings};
use crate::checked::{CheckError, CheckedMock};
use crate::ir::MockIr;
use crate::luau::{ConsumerCandidate, Contract, HookCandidate, ScriptCandidate};
use crate::store::SeedData;

pub(crate) struct SourceView<'a> {
    path: &'a str,
    coordinate: &'a str,
    kind: ActiveKind,
    source: &'a str,
}

impl SourceView<'_> {
    pub(super) fn path(&self) -> &str {
        self.path
    }
    pub(super) fn coordinate(&self) -> &str {
        self.coordinate
    }
    pub(super) fn kind(&self) -> ActiveKind {
        self.kind
    }
    pub(super) fn source(&self) -> &str {
        self.source
    }
}

pub(crate) struct ValidationView<'a> {
    pub(super) ir: &'a Arc<MockIr>,
    pub(super) layout: &'a Layout,
    pub(super) server: &'a ServerSettings,
    contract: &'a Contract,
    files: Vec<SourceView<'a>>,
    pub(super) hooks: Vec<(crate::hooks::HookSlot, &'a str)>,
    pub(super) hook_settings: crate::hooks::HookSettings,
    pub(super) clock: &'a crate::clock::ClockSettings,
    pub(super) events: &'a crate::events::EventSettings,
}

pub(crate) struct ValidationSettings<'a> {
    pub server: &'a ServerSettings,
    pub hooks: crate::hooks::HookSettings,
    pub clock: &'a crate::clock::ClockSettings,
    pub events: &'a crate::events::EventSettings,
}

impl<'a> ValidationView<'a> {
    /// Derive path, coordinate, and role from the IR so wire-provided roles and
    /// arbitrary paths never become authoritative.
    pub(crate) fn new(
        ir: &'a Arc<MockIr>,
        layout: &'a Layout,
        contract: &'a Contract,
        settings: ValidationSettings<'a>,
        sources: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, ProjectError> {
        let ValidationSettings {
            server,
            hooks: hook_settings,
            clock,
            events,
        } = settings;
        if !Arc::ptr_eq(ir, contract.ir()) {
            return Err(ProjectError::new(
                "ownership",
                "validation IR identity mismatch",
            ));
        }
        let mut inputs = BTreeMap::new();
        for (path, source) in sources {
            if inputs.insert(path, source).is_some() {
                return Err(ProjectError::new(
                    "ownership",
                    "invalid validation source inventory",
                ));
            }
        }
        if inputs.len()
            != layout.resolvers.len()
                + layout.seeds.len()
                + layout.consumers.len()
                + crate::hooks::HookSlot::ALL.len()
        {
            return Err(ProjectError::new(
                "ownership",
                "validation source coverage mismatch",
            ));
        }
        let mut files = Vec::new();
        let missing = || ProjectError::new("ownership", "validation source coverage mismatch");
        let mut hooks = Vec::new();
        for slot in crate::hooks::HookSlot::ALL {
            hooks.push((slot, inputs.remove(slot.path()).ok_or_else(missing)?));
        }
        for entry in &layout.resolvers {
            files.push(SourceView {
                path: &entry.path,
                coordinate: &entry.coordinate,
                kind: ActiveKind::Resolver(entry.script),
                source: inputs.remove(entry.path.as_str()).ok_or_else(missing)?,
            });
        }
        for entry in &layout.seeds {
            files.push(SourceView {
                path: &entry.path,
                coordinate: &ir.graph().types()[ir.models()[entry.model.0].ty.0].name,
                kind: ActiveKind::Seed(entry.model),
                source: inputs.remove(entry.path.as_str()).ok_or_else(missing)?,
            });
        }
        for (index, consumer) in layout.consumers.iter().enumerate() {
            files.push(SourceView {
                path: consumer.path(),
                coordinate: consumer.name(),
                kind: ActiveKind::Consumer(index),
                source: inputs
                    .remove(consumer.path().as_str())
                    .ok_or_else(missing)?,
            });
        }
        for (slot, source) in &hooks {
            if hook_settings.enabled(*slot) {
                files.push(SourceView {
                    path: slot.path(),
                    coordinate: slot.name(),
                    kind: ActiveKind::Hook(*slot),
                    source,
                });
            }
        }
        Ok(Self {
            ir,
            layout,
            contract,
            server,
            files,
            hooks,
            hook_settings,
            clock,
            events,
        })
    }

    pub(super) fn files(&self) -> &[SourceView<'a>] {
        &self.files
    }
    pub(super) fn contract(&self) -> &Contract {
        self.contract
    }

    pub(super) fn check_replacements(
        &self,
        replacements: &[(String, String)],
    ) -> Result<CheckedMock, ProjectError> {
        let mut candidates = Vec::new();
        let mut seeds = Vec::new();
        let mut consumers = Vec::new();
        for file in &self.files {
            let source = replacements
                .iter()
                .find(|(path, _)| path == file.path())
                .map_or(file.source(), |(_, source)| source.as_str());
            match file.kind {
                ActiveKind::Resolver(script) => candidates.push(ScriptCandidate {
                    script,
                    source: source.to_owned(),
                }),
                ActiveKind::Seed(model) => seeds.push((model, source)),
                ActiveKind::Hook(_) => {}
                ActiveKind::Consumer(index) => consumers.push(ConsumerCandidate {
                    registration: self.layout.consumers[index].clone(),
                    source: source.to_owned(),
                }),
            }
        }
        let error = |error| super::read::check_error(self.ir, self.layout, error);
        let seed = SeedData::jsonl(self.ir, &seeds).map_err(|e| error(CheckError::Seed(e)))?;
        let hooks = self
            .hooks
            .iter()
            .filter(|(slot, _)| self.hook_settings.enabled(*slot))
            .map(|(slot, source)| HookCandidate {
                slot: *slot,
                source: replacements
                    .iter()
                    .find(|(path, _)| path == slot.path())
                    .map_or(*source, |(_, source)| source.as_str())
                    .to_owned(),
            })
            .collect();
        CheckedMock::check_with_options(
            self.server.clone(),
            self.ir.clone(),
            candidates,
            seed,
            crate::checked::CheckOptions {
                hook_settings: self.hook_settings,
                state: self.contract.state().clone(),
                hooks,
                clock: self.clock.clone(),
                events: self.events.clone(),
                consumers,
            },
        )
        .map_err(error)
    }
}

impl ActiveSnapshot {
    pub(crate) fn validation(&self) -> Result<ValidationView<'_>, ProjectError> {
        ValidationView::new(
            self.schema.ir(),
            &self.layout,
            self.contract(),
            ValidationSettings {
                server: &self.schema.settings().server,
                hooks: self.schema.settings().hooks,
                clock: &self.schema.settings().clock,
                events: &self.schema.settings().events,
            },
            self.owned_sources(),
        )
    }
}
