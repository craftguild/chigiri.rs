//! Parse the closed v11 bundle envelope shared by server, seed, resolver, and hook setup.

mod envelope;
#[cfg(test)]
mod envelope_tests;
mod origins;
#[cfg(test)]
mod simplified_tests;

use crate::diagnostic::Diagnostic;
use chigiri_mock::checked::{CheckError, CheckedMock};
use chigiri_mock::luau::ScriptCandidate;
use chigiri_mock::store::SeedData;
pub(crate) use envelope::Envelope;
pub(crate) use origins::Origins;
use std::path::Path;
use std::sync::Arc;

pub(crate) use chigiri_mock::limits::CONFIG_BYTES;

pub(crate) struct Config {
    pub checked: CheckedMock,
    pub origins: Arc<Origins>,
}

impl Config {
    pub fn project(path: &Path) -> Result<Self, Diagnostic> {
        Self::project_observed(path, |_| {})
    }

    pub fn project_observed(
        path: &Path,
        observed: impl FnOnce(&chigiri_mock::ir::MockIr),
    ) -> Result<Self, Diagnostic> {
        let checked = chigiri_mock::project::read_project_observed(path, observed)
            .map_err(Diagnostic::project)?;
        let origins = Arc::new(Origins::project(checked.scripts().contract().ir().clone())?);
        Ok(Self { checked, origins })
    }

    pub fn read(path: &Path) -> Result<Self, Diagnostic> {
        Self::read_observed(path, |_| {})
    }

    pub fn read_observed(
        path: &Path,
        observed: impl FnOnce(&chigiri_mock::ir::MockIr),
    ) -> Result<Self, Diagnostic> {
        let source = crate::files::read(path, CONFIG_BYTES)?;
        let config = Self::parse_observed(path, source.source(), observed)?;
        source.verify()?;
        Ok(config)
    }

    #[cfg(test)]
    pub fn parse(path: &Path, source: &str) -> Result<Self, Diagnostic> {
        Self::parse_observed(path, source, |_| {})
    }

    fn parse_observed(
        path: &Path,
        source: &str,
        observed: impl FnOnce(&chigiri_mock::ir::MockIr),
    ) -> Result<Self, Diagnostic> {
        let envelope = Envelope::parse(path, source)?;
        let Envelope {
            server,
            ir,
            seed,
            seed_position,
            mut resolvers,
            origins,
            hook_settings,
            state,
            hook_candidates,
            clock,
            events,
            consumer_candidates,
        } = envelope;
        observed(&ir);
        let seed_error = |message| {
            let mut failure = Diagnostic::new(path, "seed", message);
            failure.set_position(seed_position);
            failure
        };
        let seed = SeedData::json(&seed).map_err(|error| seed_error(error.message))?;
        let mut candidates = Vec::new();
        for (id, plan) in ir.scripts().iter().enumerate() {
            let field = &ir.graph().fields()[plan.field.0];
            let coordinate = format!("{}.{}", ir.graph().types()[field.parent.0].name, field.name);
            let entry = resolvers
                .remove(&coordinate)
                .ok_or_else(|| origins.coverage())?;
            candidates.push(ScriptCandidate {
                script: chigiri_mock::ir::ScriptId(id),
                source: entry.luau,
            });
        }
        let checked = CheckedMock::check_with_options(
            server,
            ir,
            candidates,
            seed,
            chigiri_mock::checked::CheckOptions {
                hook_settings,
                state,
                hooks: hook_candidates,
                clock,
                events,
                consumers: consumer_candidates,
            },
        )
        .map_err(|error| match error {
            CheckError::Settings(error) => Diagnostic::new(path, "validation", error.message),
            CheckError::Script(error) => origins.script(None, error),
            CheckError::Seed(error) => seed_error(error.message),
            CheckError::Limit(message) => Diagnostic::new(path, "limit", message),
            CheckError::Environment(message) => Diagnostic::environment(message),
        })?;
        Ok(Self { checked, origins })
    }
}
