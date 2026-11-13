//! Render initial candidates and management metadata from IR, then validate types and module initialization before deployment.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::digest::Digest;
use super::metadata::{self, Entries};
use super::normalize::{AdoptedSchema, normalized_sources};
use super::{Layout, ProjectError, state, summary};
use crate::hooks::{HookSettings, StateContract};
use crate::ir::MockIr;
use crate::luau::{Contract, ScriptSet, static_candidates};
use chigiri_host::PortableRelativePath;

/// Generated files and ownership metadata for a new project.
/// The caller creates configuration separately because it owns the explicit SDL references.
pub struct GeneratedProject {
    /// Canonical project-relative paths and complete contents, with no permission to overwrite existing files.
    pub files: BTreeMap<PortableRelativePath, String>,
    pub(super) state: state::State,
    pub(super) snapshot: state::Snapshot,
}

/// Generate project-owned bytes without creating or discovering files.
pub fn generate_project(ir: Arc<MockIr>) -> Result<GeneratedProject, ProjectError> {
    generate_with(
        ir,
        HookSettings::default(),
        StateContract::default(),
        Default::default(),
        Default::default(),
    )
}

pub(super) fn generate_with(
    ir: Arc<MockIr>,
    hooks: HookSettings,
    state: StateContract,
    clock: crate::clock::ClockSettings,
    events: crate::events::EventSettings,
) -> Result<GeneratedProject, ProjectError> {
    generate_observed(ir, hooks, state, clock, events, || {})
}

pub(super) fn generate_observed(
    ir: Arc<MockIr>,
    hooks: HookSettings,
    state: StateContract,
    clock: crate::clock::ClockSettings,
    events: crate::events::EventSettings,
    checking: impl FnOnce() + Send + 'static,
) -> Result<GeneratedProject, ProjectError> {
    let ir = canonical_ir(&ir)?;
    super::config_events::validate(&events, &ir)?;
    let layout = Layout::with_events(&ir, &events)?;
    let pending = std::thread::Builder::new()
        .name("chigiri-mock-generate".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let contract = Contract::with_state(ir.clone(), state).map_err(script_error)?;
            let candidates = static_candidates(&ir).map_err(script_error)?;
            let hook_candidates = crate::luau::static_hooks().map_err(script_error)?;
            let consumer_candidates =
                crate::luau::static_consumers(&ir, &events).map_err(script_error)?;
            let used = contract.check_capacity(&candidates).map_err(script_error)?;
            contract
                .hook_capacity(
                    hook_candidates
                        .iter()
                        .map(|candidate| (candidate.slot, candidate.source.as_str())),
                    crate::luau::MAX_GENERATED - used,
                )
                .map_err(script_error)?;
            checking();
            let (scripts, checked_hooks, checked_consumers) = ScriptSet::with_modules(
                contract,
                candidates,
                hook_candidates,
                consumer_candidates,
                None,
            )
            .map_err(script_error)?;
            generate(
                &layout,
                &scripts,
                hooks,
                &checked_hooks,
                &checked_consumers,
                &clock,
                &events,
            )
        })
        .map_err(|_| environment())?;
    pending.join().map_err(|_| environment())?
}

fn generate(
    layout: &Layout,
    scripts: &ScriptSet,
    settings: HookSettings,
    checked_hooks: &[crate::luau::HookScript],
    checked_consumers: &[crate::luau::ConsumerScript],
    clock: &crate::clock::ClockSettings,
    events: &crate::events::EventSettings,
) -> Result<GeneratedProject, ProjectError> {
    let ir = scripts.contract().ir();
    let types = scripts.contract().declarations().to_owned();
    let summary = summary::build(ir)?;
    let mut files = BTreeMap::new();
    let mut resolvers = Vec::new();
    for entry in &layout.resolvers {
        let source = scripts.scripts()[entry.script.0].source().to_owned();
        let base = Digest::bytes(source.as_bytes());
        resolvers.push(state::Resolver {
            coordinate: entry.coordinate.clone(),
            path: entry.path.clone(),
            base: base.clone(),
            candidate: base,
            dist: None,
            active: true,
        });
        files.insert(entry.path.clone(), source);
    }
    let mut seeds = Vec::new();
    for entry in &layout.seeds {
        seeds.push(state::Seed {
            model: ir.graph().types()[ir.models()[entry.model.0].ty.0]
                .name
                .to_string(),
            path: entry.path.clone(),
            active: true,
        });
        files.insert(entry.path.clone(), String::new());
    }
    let mut hooks = Vec::new();
    for script in checked_hooks {
        let slot = script.slot();
        let digest = Digest::bytes(script.source().as_bytes());
        hooks.push(state::Hook {
            slot,
            path: super::logical(slot.path())?,
            base: digest.clone(),
            candidate: digest,
            dist: None,
        });
        files.insert(super::logical(slot.path())?, script.source().to_owned());
    }
    let config =
        super::config_stamp::ConfigStamp::new(settings, scripts.contract().state(), clock, events)?;
    let mut consumers = Vec::new();
    for consumer in checked_consumers {
        let registration = consumer.registration();
        let digest = Digest::bytes(consumer.source().as_bytes());
        consumers.push(state::Consumer {
            name: registration.name().to_owned(),
            field: ir.graph().fields()[registration.field().0].name.to_string(),
            path: registration.path().clone(),
            base: digest.clone(),
            candidate: digest,
            dist: None,
            active: true,
        });
        files.insert(registration.path().clone(), consumer.source().to_owned());
    }
    let mut state = state::State {
        format: state::FORMAT.into(),
        generation: Digest::bytes(b""),
        public_digest: Digest::bytes(b""),
        schema_digest: ir.graph().source().digest().to_hex(),
        types_digest: Digest::bytes(types.as_bytes()),
        summary_digest: Digest::bytes(metadata::render(&summary, metadata::MAX_BYTES)?.as_bytes()),
        snapshot_digest: Digest::bytes(b""),
        config: config.clone(),
        hooks: Entries(hooks),
        resolvers: Entries(resolvers),
        seeds: Entries(seeds),
        consumers: Entries(consumers),
    };
    let mut snapshot = state::Snapshot {
        format: state::SNAPSHOT_FORMAT.into(),
        generation: state.generation.clone(),
        public_schema: AdoptedSchema::from_sources(ir.public_sources())?,
        schema_digest: state.schema_digest.clone(),
        summary,
        config,
    };
    let (snapshot_bytes, state_bytes) = state.render_documents(&mut snapshot, &types)?;
    files.insert(super::fixed_path("generated/types.d.luau"), types);
    files.insert(
        super::fixed_path("generated/schema.snapshot.json"),
        snapshot_bytes,
    );
    files.insert(
        super::fixed_path("generated/update-state.json"),
        state_bytes,
    );
    Ok(GeneratedProject {
        files,
        state,
        snapshot,
    })
}

fn canonical_ir(ir: &MockIr) -> Result<Arc<MockIr>, ProjectError> {
    let public = normalized_sources("schema", ir.public_sources())?;
    let storage = normalized_sources("storage", ir.storage_sources())?;
    MockIr::parse_with_storage(public, storage)
        .map(Arc::new)
        .map_err(|error| ProjectError::new("schema", error.code))
}

fn script_error(error: crate::luau::ScriptError) -> ProjectError {
    let mut failure = ProjectError::new(error.phase, error.message);
    if let Some(position) = error.source_position() {
        failure.set_position(position);
    }
    failure
}
fn environment() -> ProjectError {
    let mut error = ProjectError::new("environment", "project generation worker failed");
    error.exit = 2;
    error
}
