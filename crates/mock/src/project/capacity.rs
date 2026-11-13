//! Check aggregate active-input capacity independently of earlier resolver or seed conformance errors.

use std::collections::BTreeMap;

use super::{ActiveKind, ActiveSnapshot, ProjectError};
use crate::checked::CheckError;
use crate::luau::ScriptCandidate;
use crate::store::SeedData;

impl super::validation::ValidationView<'_> {
    pub(super) fn check_capacity(
        &self,
        sources: &BTreeMap<&str, &str>,
    ) -> Result<(), ProjectError> {
        let error = |error| super::read::check_error(self.ir, self.layout, error);
        let source = |file: &super::validation::SourceView| {
            sources
                .get(file.path())
                .copied()
                .unwrap_or(file.source())
                .to_owned()
        };
        let candidates: Vec<_> = self
            .files()
            .iter()
            .filter_map(|file| match file.kind() {
                ActiveKind::Resolver(script) => Some(ScriptCandidate {
                    script,
                    source: source(file),
                }),
                ActiveKind::Seed(_) | ActiveKind::Hook(_) | ActiveKind::Consumer(_) => None,
            })
            .collect();
        let generated = self
            .contract()
            .check_capacity(&candidates)
            .map_err(|e| error(CheckError::Script(e)))?;
        let hook_bytes = self
            .contract()
            .hook_capacity(
                self.hooks.iter().map(|(slot, source)| {
                    (*slot, sources.get(slot.path()).copied().unwrap_or(source))
                }),
                crate::luau::MAX_GENERATED - generated,
            )
            .map_err(|e| error(CheckError::Script(e)))?;
        let consumers = self
            .files()
            .iter()
            .filter_map(|file| match file.kind() {
                ActiveKind::Consumer(index) => Some(crate::luau::ConsumerCandidate {
                    registration: self.layout.consumers[index].clone(),
                    source: source(file),
                }),
                _ => None,
            })
            .collect();
        self.contract()
            .prepare_consumers(
                consumers,
                crate::luau::MAX_GENERATED - generated - hook_bytes,
            )
            .map_err(|e| error(CheckError::Script(e)))?;
        let seeds: Vec<_> = self
            .files()
            .iter()
            .filter_map(|file| match file.kind() {
                ActiveKind::Seed(model) => Some((
                    model,
                    sources.get(file.path()).copied().unwrap_or(file.source()),
                )),
                ActiveKind::Resolver(_) | ActiveKind::Hook(_) | ActiveKind::Consumer(_) => None,
            })
            .collect();
        SeedData::check_capacity(self.ir, &seeds).map_err(|e| {
            let mut failure = error(CheckError::Seed(e));
            failure.phase = "limit";
            failure
        })?;
        // Canonicalization may reject malformed seed JSON before full readiness validation.
        // Leave non-capacity contract failures to the readiness validation path.
        let canonical = SeedData::capacity_canonical(self.ir, &seeds).map_err(|e| {
            let mut failure = error(CheckError::Seed(e));
            failure.phase = "limit";
            failure
        })?;
        // Render the complete active set with the production serializer to enforce its exact limit.
        crate::bundle::render_sources(
            self.server,
            self.ir,
            &canonical,
            self.files().iter().filter_map(|file| match file.kind() {
                ActiveKind::Resolver(_) => Some((
                    file.coordinate(),
                    sources.get(file.path()).copied().unwrap_or(file.source()),
                )),
                ActiveKind::Seed(_) | ActiveKind::Hook(_) | ActiveKind::Consumer(_) => None,
            }),
            &crate::bundle::HookSources {
                settings: self.hook_settings,
                state: self.contract().state(),
                modules: self
                    .hooks
                    .iter()
                    .filter(|(slot, _)| self.hook_settings.enabled(*slot))
                    .map(|(slot, source)| {
                        (*slot, sources.get(slot.path()).copied().unwrap_or(source))
                    })
                    .collect(),
            },
            self.clock,
            &crate::bundle::EventSources {
                settings: self.events,
                modules: self
                    .files()
                    .iter()
                    .filter_map(|file| match file.kind() {
                        ActiveKind::Consumer(_) => Some((
                            file.coordinate(),
                            sources.get(file.path()).copied().unwrap_or(file.source()),
                        )),
                        _ => None,
                    })
                    .collect(),
            },
        )
        .map_err(|message| error(CheckError::Limit(message)))?;
        Ok(())
    }
}

impl ActiveSnapshot {
    pub(super) fn check_capacity(
        &self,
        sources: &BTreeMap<&str, &str>,
    ) -> Result<(), ProjectError> {
        self.validation()?.check_capacity(sources)
    }
}
