//! Validate the closed portable envelope and embedded schemas without building a store or Luau VM.

use super::{CONFIG_BYTES, Origins};
use crate::diagnostic::{Diagnostic, position};
use chigiri::core::{SchemaDigest, Source};
use chigiri_host::PortableRelativePath;
use chigiri_mock::hooks::{HookSettings, HookSlot, Registration, StateContract};
use chigiri_mock::ir::MockIr;
use chigiri_mock::luau::HookCandidate;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    format: String,
    capabilities: String,
    server: Server,
    schema: Schema,
    storage: Storage,
    clock: Clock,
    seed: Seed,
    resolvers: BTreeMap<String, Resolver>,
    hooks: Hooks,
    state: StateJson,
    events: EventJson,
    consumers: BTreeMap<String, Resolver>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Hooks {
    request: Registration,
    operation: Registration,
    field: Registration,
    modules: BTreeMap<HookSlot, Resolver>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateJson {
    contract_json: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventJson {
    settings_json: String,
}

pub(crate) use chigiri_mock::project::ServerSettings as Server;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Schema {
    digest: String,
    vocabulary: String,
    sources: Vec<SchemaSource>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemaSource {
    name: PortableRelativePath,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Storage {
    digest: String,
    #[serde(default)]
    sources: Vec<SchemaSource>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Clock {
    mode: String,
    now: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Seed {
    records_json: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Resolver {
    pub luau: String,
}

pub(crate) struct Envelope {
    pub server: Server,
    pub ir: Arc<MockIr>,
    pub(super) seed: String,
    pub(super) seed_position: (usize, usize),
    pub(super) resolvers: BTreeMap<String, Resolver>,
    pub origins: Arc<Origins>,
    pub hook_settings: HookSettings,
    pub state: StateContract,
    pub hook_candidates: Vec<HookCandidate>,
    pub clock: chigiri_mock::clock::ClockSettings,
    pub events: chigiri_mock::events::EventSettings,
    pub consumer_candidates: Vec<chigiri_mock::luau::ConsumerCandidate>,
}

impl Envelope {
    pub fn parse(path: &Path, source: &str) -> Result<Self, Diagnostic> {
        let mut base = Diagnostic::new(path, "parse", "invalid or incomplete closed TOML");
        if source.len() > CONFIG_BYTES {
            return Err(base);
        }
        let raw: RawConfig = toml_edit::de::from_str(source).map_err(|error| {
            let mut error_diag = base.clone();
            if let Some(span) = error.span() {
                error_diag.set_position(position(source, span.start));
            }
            error_diag
        })?;
        base.phase = "validation";
        if raw.format != chigiri_mock::info::CONFIG_FORMAT
            || raw.capabilities != chigiri_mock::info::CAPABILITY_VERSION
        {
            base.message = "unsupported format";
            return Err(base);
        }
        let document = source
            .parse::<toml_edit::ImDocument<String>>()
            .map_err(|_| base.clone())?;
        let digest = SchemaDigest::from_hex(&raw.schema.digest)
            .map_err(|_| Diagnostic::new(path, "schema", "invalid schema digest"))?;
        let storage_digest = SchemaDigest::from_hex(&raw.storage.digest)
            .map_err(|_| Diagnostic::new(path, "storage", "invalid storage digest"))?;
        let clock = match (raw.clock.mode.as_str(), raw.clock.now) {
            ("system", None) => chigiri_mock::clock::ClockSettings::system(),
            ("fixed", Some(now)) => chigiri_mock::clock::ClockSettings::fixed(now)
                .map_err(|_| Diagnostic::new(path, "clock", "invalid fixed clock"))?,
            _ => return Err(Diagnostic::new(path, "clock", "invalid clock settings")),
        };
        let public_source_count = raw.schema.sources.len().saturating_sub(1);
        let storage_source_count = raw.storage.sources.len();
        let ir = Arc::new(
            MockIr::from_snapshots(
                raw.schema
                    .sources
                    .into_iter()
                    .map(|source| Source::new(source.name.to_string(), source.text))
                    .collect(),
                digest,
                raw.storage
                    .sources
                    .into_iter()
                    .map(|source| Source::new(source.name.to_string(), source.text))
                    .collect(),
                storage_digest,
                &raw.schema.vocabulary,
            )
            .map_err(|error| {
                let mut failure = Diagnostic::new(path, "schema", error.code);
                let entry = error.span.and_then(|span| {
                    let index = span.source_id.index();
                    if index < public_source_count {
                        document["schema"]["sources"]
                            .as_array_of_tables()?
                            .get(index)
                    } else if index < public_source_count + storage_source_count {
                        document["storage"]["sources"]
                            .as_array_of_tables()?
                            .get(index - public_source_count)
                    } else {
                        document["schema"]["sources"]
                            .as_array_of_tables()?
                            .get(public_source_count)
                    }
                });
                if let Some(span) = entry.and_then(|entry| entry["text"].span()) {
                    failure.set_position(position(source, span.start));
                }
                failure
            })?,
        );
        let origins = Arc::new(Origins::new(path, source, &document, ir.clone()));
        let hook_settings = HookSettings {
            request: raw.hooks.request,
            operation: raw.hooks.operation,
            field: raw.hooks.field,
        };
        let expected: std::collections::BTreeSet<_> = HookSlot::ALL
            .into_iter()
            .filter(|slot| hook_settings.enabled(*slot))
            .collect();
        if expected != raw.hooks.modules.keys().copied().collect() {
            return Err(Diagnostic::new(
                path,
                "validation",
                "enabled hook coverage mismatch",
            ));
        }
        let state = StateContract::from_json(
            &raw.state.contract_json,
            &mut chigiri_mock::value::ValueBudget::default(),
        )
        .map_err(|_| Diagnostic::new(path, "state", "invalid state contract"))?;
        let hook_candidates = raw
            .hooks
            .modules
            .into_iter()
            .map(|(slot, entry)| HookCandidate {
                slot,
                source: entry.luau,
            })
            .collect();
        let events: chigiri_mock::events::EventSettings =
            serde_json::from_str(&raw.events.settings_json)
                .map_err(|_| Diagnostic::new(path, "events", "invalid event settings"))?;
        let registrations = events
            .resolve(&ir)
            .map_err(|_| Diagnostic::new(path, "events", "invalid consumer field"))?;
        let mut modules = raw.consumers;
        if modules.len() != registrations.len() {
            return Err(Diagnostic::new(
                path,
                "events",
                "consumer module coverage mismatch",
            ));
        }
        let consumer_candidates = registrations
            .into_iter()
            .map(|registration| {
                let module = modules.remove(registration.name()).ok_or_else(|| {
                    Diagnostic::new(path, "events", "consumer module coverage mismatch")
                })?;
                Ok(chigiri_mock::luau::ConsumerCandidate {
                    registration,
                    source: module.luau,
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        if raw.resolvers.len() != ir.scripts().len() {
            return Err(origins.coverage());
        }
        for plan in ir.scripts() {
            let field = &ir.graph().fields()[plan.field.0];
            let coordinate = format!("{}.{}", ir.graph().types()[field.parent.0].name, field.name);
            if !raw.resolvers.contains_key(&coordinate) {
                return Err(origins.coverage());
            }
        }
        let seed_position = position(
            source,
            document["seed"]["records_json"]
                .span()
                .map_or(0, |span| span.start),
        );
        Ok(Self {
            server: raw.server,
            ir,
            seed: raw.seed.records_json,
            seed_position,
            resolvers: raw.resolvers,
            origins,
            hook_settings,
            state,
            hook_candidates,
            clock,
            events,
            consumer_candidates,
        })
    }
}
