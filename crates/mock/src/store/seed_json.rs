//! Use borrowed `RawValue` boundaries and reject duplicate keys, excessive depth, and invalid sequences before deployment.

use std::fmt;
use std::sync::Arc;

use chigiri::core::{Object, Value};
use serde::Deserializer;
use serde::de::{Error, MapAccess, SeqAccess, Visitor};
use serde_json::value::RawValue;

use super::{MODEL_RECORDS, Store, StoreError};
use crate::ir::MockIr;
use crate::value::{MAX_BYTES, MAX_ITEMS, ValueBudget, ValueError};

use crate::limits::SEED_BYTES;

impl Store {
    /// Validate all seed data from the inline JSON string without reading files or the network.
    pub fn from_seed_json(ir: Arc<MockIr>, json: &str) -> Result<Self, StoreError> {
        let seed = object(json)?;
        Self::seeded(ir, &seed)
    }
}

pub(super) fn object(json: &str) -> Result<Object, StoreError> {
    if json.len() > SEED_BYTES {
        return Err(ValueError("seed JSON byte limit"));
    }
    let raw: &RawValue = serde_json::from_str(json).map_err(|_| invalid())?;
    let value = decode(raw, Stage::Seed, &mut ValueBudget::default(), &mut 0)?;
    let Value::Object(object) = value else {
        return Err(invalid());
    };
    Ok(object)
}

pub(super) fn record(json: &str, budget: &mut ValueBudget) -> Result<Value, StoreError> {
    let raw: &RawValue = serde_json::from_str(json).map_err(|_| invalid())?;
    if !raw.get().starts_with('{') {
        return Err(ValueError("seed record requires an object"));
    }
    decode(raw, Stage::Value(1), budget, &mut 0)
}

#[derive(Clone, Copy)]
enum Stage {
    Seed,
    Records,
    Value(usize),
}

impl Stage {
    fn depth(self) -> usize {
        match self {
            Self::Value(depth) => depth,
            _ => 0,
        }
    }
    fn child(self) -> Self {
        match self {
            Self::Seed => Self::Records,
            Self::Records => Self::Value(1),
            Self::Value(depth) => Self::Value(depth + 1),
        }
    }
}

fn invalid() -> StoreError {
    ValueError("invalid seed JSON")
}

fn decode(
    raw: &RawValue,
    stage: Stage,
    budget: &mut ValueBudget,
    records: &mut usize,
) -> Result<Value, StoreError> {
    if let Stage::Value(depth) = stage {
        // Nested values use the shared decoder and its aggregate and per-string byte limits.
        return crate::value::json::decode(raw, budget, depth, MAX_BYTES);
    }
    let text = raw.get();
    let first = text.as_bytes()[0];
    if (matches!(stage, Stage::Seed) && first != b'{')
        || (matches!(stage, Stage::Records) && first != b'[')
    {
        return Err(invalid());
    }
    budget.container(stage.depth())?;
    let mut decoder = serde_json::Deserializer::from_str(text);
    let visitor = Container {
        stage,
        budget,
        records,
    };
    let value = if first == b'{' {
        decoder.deserialize_map(visitor)
    } else {
        decoder.deserialize_seq(visitor)
    }
    .map_err(|_| invalid())?;
    decoder.end().map_err(|_| invalid())?;
    Ok(value)
}

struct Container<'a> {
    stage: Stage,
    budget: &'a mut ValueBudget,
    records: &'a mut usize,
}

impl<'de> Visitor<'de> for Container<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded JSON container")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let limit = if matches!(self.stage, Stage::Records) {
            MODEL_RECORDS
        } else {
            MAX_ITEMS
        };
        let mut values = Vec::new();
        while let Some(raw) = sequence.next_element::<&RawValue>()? {
            if values.len() == limit {
                return Err(A::Error::custom("seed array limit"));
            }
            if matches!(self.stage, Stage::Records) {
                if *self.records == super::TOTAL_RECORDS {
                    return Err(A::Error::custom("total seed record limit"));
                }
                *self.records += 1;
            }
            values.push(
                decode(raw, self.stage.child(), self.budget, self.records)
                    .map_err(A::Error::custom)?,
            );
        }
        Ok(Value::List(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut mapping: A) -> Result<Value, A::Error> {
        let mut object = Object::new();
        while let Some(key) = mapping.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(A::Error::custom("duplicate seed JSON key"));
            }
            if matches!(self.stage, Stage::Seed) && object.len() == 64 {
                return Err(A::Error::custom("seed model limit"));
            }
            self.budget.key(&key).map_err(A::Error::custom)?;
            let raw: &RawValue = mapping.next_value()?;
            let value = decode(raw, self.stage.child(), self.budget, self.records)
                .map_err(A::Error::custom)?;
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}
