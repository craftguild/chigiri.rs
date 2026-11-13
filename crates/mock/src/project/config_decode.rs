//! Decode the closed configuration shape incrementally without materializing unknown nested tables.

use std::collections::BTreeMap;

use luaur::{Function, Lua, Table, Value};

use super::{ProjectError, ProjectSettings, ServerSettings};
use crate::value::ValueBudget;

pub(super) fn decode(
    lua: &Lua,
    value: Value,
    length: Function,
    expected_format: &str,
) -> Result<ProjectSettings, ProjectError> {
    let mut decoder = Decoder {
        length,
        budget: ValueBudget::default(),
        state_scan: Default::default(),
    };
    let [format, schema, storage, clock, server, hooks, state, events] = decoder.fields_optional(
        value,
        [
            "format", "schema", "storage", "clock", "server", "hooks", "state", "events",
        ],
        7,
        1,
    )?;
    let format = decoder.string(format, 2)?;
    if format != expected_format {
        return Err(invalid());
    }
    let [sources] = decoder.fields(schema, ["sources"], 2)?;
    let sources = decoder.strings(sources, crate::limits::SDL_SOURCES)?;
    if sources.is_empty() || sources.iter().any(|value| !valid_source(value)) {
        return Err(ProjectError::new(
            "config",
            "explicit nonempty SDL paths required",
        ));
    }
    let [storage_sources] = decoder.fields(storage, ["sources"], 2)?;
    let storage_sources = decoder.strings(storage_sources, crate::limits::SDL_SOURCES)?;
    if storage_sources
        .iter()
        .any(|value| chigiri_host::PortableRelativePath::new(value).is_err())
    {
        return Err(ProjectError::new(
            "config",
            "explicit storage SDL paths required",
        ));
    }
    let clock = decode_clock(lua, &mut decoder, clock)?;
    let [listen, origins] = decoder.fields(server, ["listen", "allowed_origins"], 2)?;
    let server = ServerSettings {
        listen: decoder.string(listen, 3)?.parse().map_err(|_| invalid())?,
        allowed_origins: decoder.strings(origins, crate::value::MAX_NODES)?,
    };
    server.validate()?;
    let hooks = decoder.hooks(hooks)?;
    let state = decoder.state(state)?;
    let events = decoder.events(events)?;
    Ok(ProjectSettings {
        sources,
        storage_sources,
        clock,
        server,
        hooks,
        state,
        events,
    })
}

fn decode_clock(
    lua: &Lua,
    decoder: &mut Decoder,
    value: Value,
) -> Result<crate::clock::ClockSettings, ProjectError> {
    let table = decoder.table(value, 2)?;
    let mode = table.raw_get("mode").map_err(|_| invalid())?;
    match decoder.string(mode, 3)?.as_str() {
        "system" => {
            decoder.fields(Value::Table(table), ["mode"], 2)?;
            Ok(crate::clock::ClockSettings::system())
        }
        "fixed" => {
            let [_, now] = decoder.fields(Value::Table(table), ["mode", "now"], 2)?;
            let now = integer(now)?;
            crate::clock::ClockSettings::fixed_in(lua, now).map_err(|_| invalid())
        }
        _ => Err(invalid()),
    }
}

pub(super) fn integer(value: Value) -> Result<i64, ProjectError> {
    match value {
        Value::Integer(value) => Ok(value),
        Value::Number(value)
            if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value < -(i64::MIN as f64) =>
        {
            Ok(value as i64)
        }
        _ => Err(invalid()),
    }
}

pub(super) fn valid_source(value: &str) -> bool {
    !value.is_empty() && !value.contains(['\0', '*', '?', '[', ']']) && !value.contains("://")
}

pub(super) struct Decoder {
    // Keep the closure inside the VM and destroy it when decoding finishes.
    length: Function,
    pub(super) budget: ValueBudget,
    // Track shared state tables so the same empty slot is not scanned twice; VM handles live only during decoding.
    pub(super) state_scan: crate::codec::scan::Scan,
}

impl Decoder {
    pub(super) fn table(&mut self, value: Value, depth: usize) -> Result<Table, ProjectError> {
        self.budget.container(depth).map_err(|_| limit())?;
        let Value::Table(table) = value else {
            return Err(invalid());
        };
        if table.metatable().is_some() {
            return Err(invalid());
        }
        Ok(table)
    }

    pub(super) fn fields<const N: usize>(
        &mut self,
        value: Value,
        names: [&str; N],
        depth: usize,
    ) -> Result<[Value; N], ProjectError> {
        self.fields_optional(value, names, N, depth)
    }

    /// Decode closed fields, requiring the first `required` entries and
    /// representing absent trailing optional entries as Nil without copying
    /// unknown nested data. Callers own the meaning of each optional value.
    pub(super) fn fields_optional<const N: usize>(
        &mut self,
        value: Value,
        names: [&str; N],
        required: usize,
        depth: usize,
    ) -> Result<[Value; N], ProjectError> {
        let table = self.table(value, depth)?;
        let mut values = std::array::from_fn(|_| Value::Nil);
        let mut count = 0;
        for pair in table.pairs::<Value, Value>() {
            let (key, value) = pair.map_err(|_| invalid())?;
            count += 1;
            if count > N {
                return Err(invalid());
            }
            let key = self.string(key, depth + 1)?;
            let index = names
                .iter()
                .position(|name| *name == key)
                .ok_or_else(invalid)?;
            values[index] = value;
        }
        if values[..required]
            .iter()
            .any(|value| matches!(value, Value::Nil))
        {
            return Err(invalid());
        }
        Ok(values)
    }

    pub(super) fn string(&mut self, value: Value, depth: usize) -> Result<String, ProjectError> {
        self.string_limited(value, depth, crate::value::MAX_BYTES)
    }

    pub(super) fn string_limited(
        &mut self,
        value: Value,
        depth: usize,
        maximum: usize,
    ) -> Result<String, ProjectError> {
        let Value::String(value) = value else {
            return Err(invalid());
        };
        // Measure through Luau before `to_str` allocates the Rust copy, then reserve its budget.
        let bytes = self
            .length
            .call::<usize>(value.clone())
            .map_err(|_| invalid())?;
        if bytes > maximum {
            return Err(limit());
        }
        self.budget.string_node(bytes, depth).map_err(|_| limit())?;
        value.to_str().map_err(|_| invalid())
    }

    fn strings(&mut self, value: Value, maximum: usize) -> Result<Vec<String>, ProjectError> {
        let table = self.table(value, 3)?;
        let mut indexed = BTreeMap::new();
        for pair in table.pairs::<Value, Value>() {
            let (key, value) = pair.map_err(|_| invalid())?;
            if indexed.len() >= maximum {
                return Err(limit());
            }
            let Value::Integer(key) = key else {
                return Err(invalid());
            };
            let key = usize::try_from(key).map_err(|_| invalid())?;
            if key == 0 || key > maximum {
                return Err(invalid());
            }
            indexed.insert(key, self.string(value, 4)?);
        }
        if indexed.keys().copied().ne(1..=indexed.len()) {
            return Err(invalid());
        }
        Ok(indexed.into_values().collect())
    }
}

pub(super) fn invalid() -> ProjectError {
    ProjectError::new("config", "closed plain project data required")
}

pub(super) fn limit() -> ProjectError {
    ProjectError::new("limit", "config data resource limit")
}
