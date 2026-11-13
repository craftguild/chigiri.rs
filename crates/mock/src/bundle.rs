//! Render SDL, resolver modules, and seed data directly into a bounded output sink.

use std::fmt::{self, Write};

use crate::hooks::{HookSettings, HookSlot, StateContract};
use crate::luau::{HookScript, ScriptSet};
use crate::project::ServerSettings;
use crate::schema::VOCABULARY_VERSION;
use chigiri_host::PortableRelativePath;

struct Text(String);
impl Write for Text {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if text.len() > crate::limits::CONFIG_BYTES.saturating_sub(self.0.len()) {
            return Err(fmt::Error);
        }
        self.0.push_str(text);
        Ok(())
    }
}

/// Serialize one validated startup snapshot into the canonical portable bundle.
/// The caller is responsible for completing script, seed, and settings validation first.
pub fn render(
    server: &ServerSettings,
    scripts: &ScriptSet,
    seed: &str,
    settings: HookSettings,
    hooks: &[HookScript],
    clock: &crate::clock::ClockSettings,
    events: &EventSources<'_>,
) -> Result<String, &'static str> {
    render_sources(
        server,
        scripts.contract().ir(),
        seed,
        scripts
            .scripts()
            .iter()
            .map(|script| (script.coordinate(), script.source())),
        &HookSources {
            settings,
            state: scripts.contract().state(),
            modules: hooks
                .iter()
                .map(|hook| (hook.slot(), hook.source()))
                .collect(),
        },
        clock,
        events,
    )
}

pub(crate) struct HookSources<'a> {
    pub settings: HookSettings,
    pub state: &'a StateContract,
    pub modules: Vec<(HookSlot, &'a str)>,
}

/// Captured event settings and registered module bytes for portable serialization.
pub struct EventSources<'a> {
    /// Validated source policy and registration map.
    pub settings: &'a crate::events::EventSettings,
    /// Complete registered module inventory in deterministic name order.
    pub modules: Vec<(&'a str, &'a str)>,
}

/// The capacity preflight uses the same serializer.
pub(crate) fn render_sources<'a>(
    server: &ServerSettings,
    ir: &crate::ir::MockIr,
    seed: &str,
    scripts: impl Iterator<Item = (&'a str, &'a str)>,
    hooks: &HookSources<'_>,
    clock: &crate::clock::ClockSettings,
    events: &EventSources<'_>,
) -> Result<String, &'static str> {
    document(server, ir, seed, scripts, hooks, clock, events)
        .map_err(|_| "canonical bundle exceeds 256 KiB")
}

fn document<'a>(
    server: &ServerSettings,
    ir: &crate::ir::MockIr,
    seed: &str,
    scripts: impl Iterator<Item = (&'a str, &'a str)>,
    hooks: &HookSources<'_>,
    clock: &crate::clock::ClockSettings,
    events: &EventSources<'_>,
) -> Result<String, fmt::Error> {
    let mut out = Text(String::new());
    out.write_str(
        "# Generated from validated startup inputs; the bundle excludes live store state.\n",
    )?;
    entry(&mut out, "format", crate::info::CONFIG_FORMAT)?;
    entry(&mut out, "capabilities", crate::api::runtime::VERSION)?;
    out.write_str("\n[server]\n")?;
    entry(&mut out, "listen", &server.listen.to_string())?;
    out.write_str("allowed_origins = [")?;
    for (index, origin) in server.allowed_origins.iter().enumerate() {
        if origin.len() > crate::limits::CONFIG_BYTES.saturating_sub(out.0.len()) {
            return Err(fmt::Error);
        }
        if index != 0 {
            out.write_str(", ")?;
        }
        write!(out, "{}", toml_edit::Value::from(origin.as_str()))?;
    }
    out.write_str("]\n\n[schema]\n")?;
    entry(
        &mut out,
        "digest",
        &ir.public_graph().source().digest().to_hex(),
    )?;
    entry(&mut out, "vocabulary", VOCABULARY_VERSION)?;
    for source in ir.public_graph().source().sources() {
        let name = PortableRelativePath::new(source.name()).map_err(|_| fmt::Error)?;
        out.write_str("\n[[schema.sources]]\n")?;
        entry(&mut out, "name", name.as_str())?;
        entry(&mut out, "text", source.text())?;
    }
    out.write_str("\n[storage]\n")?;
    entry(&mut out, "digest", &ir.graph().source().digest().to_hex())?;
    for source in ir.storage_sources() {
        let name = PortableRelativePath::new(source.name()).map_err(|_| fmt::Error)?;
        out.write_str("\n[[storage.sources]]\n")?;
        entry(&mut out, "name", name.as_str())?;
        entry(&mut out, "text", source.text())?;
    }
    out.write_str("\n[clock]\n")?;
    entry(&mut out, "mode", clock.mode())?;
    if let Some(now) = clock.fixed_value() {
        integer_entry(&mut out, "now", now)?;
    }
    out.write_str("\n[state]\n")?;
    let state = hooks
        .state
        .to_json(crate::limits::CONFIG_BYTES.saturating_sub(out.0.len()))
        .map_err(|_| fmt::Error)?;
    entry(&mut out, "contract_json", &state)?;
    for (phase, registration) in [
        ("request", hooks.settings.request),
        ("operation", hooks.settings.operation),
        ("field", hooks.settings.field),
    ] {
        write!(
            out,
            "\n[hooks.{phase}]\nbefore = {}\nafter = {}\n",
            registration.before, registration.after
        )?;
    }
    out.write_str("\n[hooks.modules]\n")?;
    for (slot, source) in &hooks.modules {
        write!(
            out,
            "\n[hooks.modules.{}]\n",
            toml_edit::Value::from(slot.name())
        )?;
        entry(&mut out, "luau", source)?;
    }
    out.write_str("\n[events]\n")?;
    let settings = crate::project::metadata::render(
        events.settings,
        crate::limits::CONFIG_BYTES.saturating_sub(out.0.len()),
    )
    .map_err(|_| fmt::Error)?;
    entry(&mut out, "settings_json", &settings)?;
    out.write_str("\n[consumers]\n")?;
    for (name, source) in &events.modules {
        write!(out, "\n[consumers.{}]\n", toml_edit::Value::from(*name))?;
        entry(&mut out, "luau", source)?;
    }
    out.write_str("\n[seed]\n")?;
    entry(&mut out, "records_json", seed)?;
    // Emit the table header even when there are no resolvers.
    out.write_str("\n[resolvers]\n")?;
    for (coordinate, source) in scripts {
        write!(
            out,
            "\n[resolvers.{}]\n",
            toml_edit::Value::from(coordinate)
        )?;
        entry(&mut out, "luau", source)?;
    }
    Ok(out.0)
}

fn integer_entry(out: &mut Text, key: &str, value: i64) -> fmt::Result {
    writeln!(out, "{key} = {value}")
}

fn entry(out: &mut Text, key: &str, value: &str) -> fmt::Result {
    if value.len() > crate::limits::CONFIG_BYTES.saturating_sub(out.0.len()) {
        return Err(fmt::Error);
    }
    write!(out, "{key} = ")?;
    if value.contains('\n')
        && !value.contains("'''")
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        writeln!(out, "'''\n{value}'''")
    } else {
        writeln!(out, "{}", toml_edit::Value::from(value))
    }
}
