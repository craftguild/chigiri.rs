//! Validate every pre-boot input once and retain the exact values used by serving and export.

use std::fmt;
use std::sync::Arc;

use crate::hooks::{HookSettings, HookSlot, StateContract};
use crate::ir::MockIr;
use crate::luau::{Contract, HookCandidate, HookScript, ScriptCandidate, ScriptError, ScriptSet};
use crate::project::{ProjectError, ServerSettings};
use crate::store::{SeedData, SeedError, Store};

/// Failure while constructing a validated Mock runtime snapshot.
#[derive(Debug)]
pub enum CheckError {
    /// Server bind or origin settings violate the project contract.
    Settings(Box<ProjectError>),
    /// Resolver or hook generation, type checking, or initialization failed.
    Script(ScriptError),
    /// Seed values, identities, relations, or aggregate capacity are invalid.
    Seed(SeedError),
    /// Rendering the portable bundle exceeded its fixed capacity.
    Limit(&'static str),
    /// Validation worker creation or join failed.
    Environment(&'static str),
}

impl fmt::Display for CheckError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Settings(error) => write!(out, "{error}"),
            Self::Script(error) => write!(out, "{error}"),
            Self::Seed(error) => write!(out, "seed:{} {}", error.line, error.message),
            Self::Limit(message) | Self::Environment(message) => out.write_str(message),
        }
    }
}
impl std::error::Error for CheckError {}

/// Extended pre-boot inputs shared by project and single-file validation.
#[derive(Default)]
pub struct CheckOptions {
    /// Enabled lifecycle hook slots.
    pub hook_settings: HookSettings,
    /// Declared request-state contract.
    pub state: StateContract,
    /// Hook modules supplied for the enabled slots.
    pub hooks: Vec<HookCandidate>,
    /// Application clock mode embedded in the portable bundle.
    pub clock: crate::clock::ClockSettings,
    /// Source limits and every explicit consumer registration.
    pub events: crate::events::EventSettings,
    /// All registered modules, including disabled ones.
    pub consumers: Vec<crate::luau::ConsumerCandidate>,
}

/// One validated snapshot of server settings, SDL-derived modules, hooks, seed state, and bundle.
/// Serving and export consume these owned values without reopening project files or rerunning Luau.
pub struct CheckedMock {
    server: ServerSettings,
    scripts: ScriptSet,
    store: Store,
    bundle: String,
    hook_settings: HookSettings,
    hooks: Vec<HookScript>,
    clock: crate::clock::ClockSettings,
    events: crate::events::EventSettings,
    consumers: Vec<crate::luau::ConsumerScript>,
}

/// Complete owned startup snapshot, including registered consumer modules.
pub struct RuntimeInputs {
    /// Validated listener and origin policy.
    pub server: ServerSettings,
    /// Checked GraphQL resolver modules.
    pub scripts: ScriptSet,
    /// Checked enabled lifecycle hooks.
    pub hooks: Vec<HookScript>,
    /// Validated initial seed store.
    pub store: Store,
    /// Apparent-time policy, independent of source timer cadence.
    pub clock: crate::clock::ClockSettings,
    /// Validated shared source policy and registrations.
    pub events: crate::events::EventSettings,
    /// All registered checked modules, including disabled ones.
    pub consumers: Vec<crate::luau::ConsumerScript>,
}

impl CheckedMock {
    /// Validate settings, resolvers, seeds, and bundle capacity from one set of inputs.
    pub fn check(
        server: ServerSettings,
        ir: Arc<MockIr>,
        candidates: Vec<ScriptCandidate>,
        seed: SeedData,
    ) -> Result<Self, CheckError> {
        Self::check_with_options(server, ir, candidates, seed, CheckOptions::default())
    }

    /// Validate the startup snapshot together with every enabled hook module.
    pub fn check_with_hooks(
        server: ServerSettings,
        ir: Arc<MockIr>,
        candidates: Vec<ScriptCandidate>,
        seed: SeedData,
        hook_settings: HookSettings,
        state: StateContract,
        hooks: Vec<HookCandidate>,
    ) -> Result<Self, CheckError> {
        Self::check_with_options(
            server,
            ir,
            candidates,
            seed,
            CheckOptions {
                hook_settings,
                state,
                hooks,
                clock: Default::default(),
                events: Default::default(),
                consumers: Vec::new(),
            },
        )
    }

    /// Validate all pre-boot inputs with explicit stateful runtime options.
    pub fn check_with_options(
        server: ServerSettings,
        ir: Arc<MockIr>,
        candidates: Vec<ScriptCandidate>,
        seed: SeedData,
        options: CheckOptions,
    ) -> Result<Self, CheckError> {
        let CheckOptions {
            hook_settings,
            state,
            hooks,
            clock,
            events,
            consumers,
        } = options;
        server
            .validate()
            .map_err(|error| CheckError::Settings(Box::new(error)))?;
        std::thread::Builder::new()
            .name("chigiri-mock-check".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let registrations = events.resolve(&ir).map_err(|_| {
                    CheckError::Script(ScriptError::new(
                        "contract",
                        "consumer registration field mismatch",
                    ))
                })?;
                if registrations.len() != consumers.len()
                    || consumers.iter().any(|candidate| {
                        !registrations
                            .iter()
                            .any(|expected| expected == &candidate.registration)
                    })
                {
                    return Err(CheckError::Script(ScriptError::new(
                        "contract",
                        "consumer registration coverage mismatch",
                    )));
                }
                let expected: std::collections::BTreeSet<_> = HookSlot::ALL
                    .into_iter()
                    .filter(|slot| hook_settings.enabled(*slot))
                    .collect();
                let actual: std::collections::BTreeSet<_> =
                    hooks.iter().map(|hook| hook.slot).collect();
                if expected != actual || actual.len() != hooks.len() {
                    return Err(CheckError::Script(ScriptError::new(
                        "contract",
                        "enabled hook coverage mismatch",
                    )));
                }
                let contract =
                    Contract::with_state(ir.clone(), state).map_err(CheckError::Script)?;
                let used = contract
                    .check_capacity(&candidates)
                    .map_err(CheckError::Script)?;
                contract
                    .hook_capacity(
                        hooks.iter().map(|hook| (hook.slot, hook.source.as_str())),
                        crate::luau::MAX_GENERATED - used,
                    )
                    .map_err(CheckError::Script)?;
                let (scripts, hooks, consumers) =
                    ScriptSet::with_modules(contract, candidates, hooks, consumers, None)
                        .map_err(CheckError::Script)?;
                let canonical = seed.resolved_canonical(&ir).map_err(CheckError::Seed)?;
                let store = seed.store(ir).map_err(CheckError::Seed)?;
                let bundle = crate::bundle::render(
                    &server,
                    &scripts,
                    &canonical,
                    hook_settings,
                    &hooks,
                    &clock,
                    &crate::bundle::EventSources {
                        settings: &events,
                        modules: consumers
                            .iter()
                            .map(|consumer| (consumer.registration().name(), consumer.source()))
                            .collect(),
                    },
                )
                .map_err(CheckError::Limit)?;
                Ok(Self {
                    server,
                    scripts,
                    store,
                    bundle,
                    hook_settings,
                    hooks,
                    clock,
                    events,
                    consumers,
                })
            })
            .map_err(|_| CheckError::Environment("cannot create validation worker"))?
            .join()
            .map_err(|_| CheckError::Environment("validation worker failed"))?
    }

    /// Return the validated server settings.
    pub fn server(&self) -> &ServerSettings {
        &self.server
    }

    /// Return modules validated against this IR.
    pub fn scripts(&self) -> &ScriptSet {
        &self.scripts
    }

    /// Return the validated hook registration settings.
    /// Runtime hook state is stored separately and is not included here.
    pub fn hook_settings(&self) -> HookSettings {
        self.hook_settings
    }

    /// Return hook modules validated for this snapshot.
    pub fn hooks(&self) -> &[HookScript] {
        &self.hooks
    }

    /// Final portable bundle bytes derived from the checked inputs.
    pub fn bundle(&self) -> &str {
        &self.bundle
    }

    /// Event configuration validated against this snapshot's public schema.
    pub fn events(&self) -> &crate::events::EventSettings {
        &self.events
    }

    /// Every registered checked module, including disabled consumers.
    pub fn consumers(&self) -> &[crate::luau::ConsumerScript] {
        &self.consumers
    }

    /// Consume the checked snapshot and return the components needed to serve it.
    pub fn into_runtime(self) -> RuntimeInputs {
        RuntimeInputs {
            server: self.server,
            scripts: self.scripts,
            hooks: self.hooks,
            store: self.store,
            clock: self.clock,
            events: self.events,
            consumers: self.consumers,
        }
    }
}
