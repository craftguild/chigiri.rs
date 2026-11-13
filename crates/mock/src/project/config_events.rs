//! Decode optional event settings into owned validated values without retaining Lua.

use std::collections::BTreeMap;

use luaur::Value;

use super::ProjectError;
use super::config_decode::{Decoder, integer, invalid, limit};
use crate::events::{ConsumerRegistration, EventSettings, SourceLimits};

impl Decoder {
    pub(super) fn events(&mut self, value: Value) -> Result<EventSettings, ProjectError> {
        if matches!(value, Value::Nil) {
            return Ok(EventSettings::default());
        }
        let [limits, consumers] = self.fields_optional(value, ["limits", "consumers"], 0, 2)?;
        let limits = self.event_limits(limits)?;
        let consumers = self.event_consumers(consumers)?;
        EventSettings::new(limits, consumers).map_err(|_| event_settings())
    }

    fn event_limits(&mut self, value: Value) -> Result<SourceLimits, ProjectError> {
        let defaults = SourceLimits::default();
        if matches!(value, Value::Nil) {
            return Ok(defaults);
        }
        let [count, bytes, age, retained, sequence] = self.fields_optional(
            value,
            [
                "max_queued_events",
                "max_queued_bytes",
                "max_queue_age_ms",
                "max_retained_bytes",
                "max_sequence_bytes",
            ],
            0,
            3,
        )?;
        let mut number = |value, fallback| {
            self.budget.container(4).map_err(|_| limit())?;
            match value {
                Value::Nil => Ok(fallback),
                value => u64::try_from(integer(value)?).map_err(|_| event_settings()),
            }
        };
        let count = number(count, defaults.max_queued_events() as u64)?;
        let bytes = number(bytes, defaults.max_queued_bytes() as u64)?;
        let age = number(age, defaults.max_queue_age_ms())?;
        let retained = number(retained, defaults.max_retained_bytes() as u64)?;
        let sequence = number(sequence, defaults.max_sequence_bytes() as u64)?;
        let size = |value| usize::try_from(value).map_err(|_| event_settings());
        SourceLimits::new(
            size(count)?,
            size(bytes)?,
            age,
            size(retained)?,
            size(sequence)?,
        )
        .map_err(|_| event_settings())
    }

    fn event_consumers(
        &mut self,
        value: Value,
    ) -> Result<BTreeMap<String, ConsumerRegistration>, ProjectError> {
        if matches!(value, Value::Nil) {
            return Ok(BTreeMap::new());
        }
        let table = self.table(value, 3)?;
        let mut consumers = BTreeMap::new();
        for pair in table.pairs::<Value, Value>() {
            if consumers.len() == EventSettings::MAX_CONSUMERS {
                return Err(limit());
            }
            let (key, value) = pair.map_err(|_| invalid())?;
            let name = self.string_limited(key, 4, EventSettings::MAX_NAME_BYTES)?;
            let [field, enabled] = self.fields(value, ["field", "enabled"], 4)?;
            // GraphQL names already have a 255-byte schema boundary. Resolve
            // against the adopted graph later; configuration cannot adopt SDL.
            let field = self.string_limited(field, 5, 255)?;
            self.budget.container(5).map_err(|_| limit())?;
            let Value::Boolean(enabled) = enabled else {
                return Err(invalid());
            };
            consumers.insert(name, ConsumerRegistration { field, enabled });
        }
        Ok(consumers)
    }
}

pub(super) fn validate(
    settings: &EventSettings,
    ir: &crate::ir::MockIr,
) -> Result<(), ProjectError> {
    settings.resolve(ir).map(|_| ()).map_err(|_| {
        ProjectError::new(
            "config",
            "consumer requires an adopted Subscription field and portable module name",
        )
    })
}

fn event_settings() -> ProjectError {
    ProjectError::new("config", "invalid event limits or consumer registration")
}
