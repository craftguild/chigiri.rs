//! Decode JSON-equivalent borrowed subtrees under shared depth, duplicate-key, and copy limits.

use std::fmt;

use chigiri::core::{Object, Value};
use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
use serde::{Deserializer, de};
use serde_json::value::RawValue;

use super::{MAX_DEPTH, MAX_ITEMS, MAX_STRING_BYTES, ValueBudget, ValueError, exact_number};

pub(crate) fn decode(
    raw: &RawValue,
    budget: &mut ValueBudget,
    depth: usize,
    key_limit: usize,
) -> Result<Value, ValueError> {
    if depth > MAX_DEPTH {
        return Err(ValueError("JSON value depth limit"));
    }
    let text = raw.get();
    let first = text.as_bytes()[0];
    let mut decoder = serde_json::Deserializer::from_str(text);
    let value = match first {
        b'{' | b'[' => {
            budget.container(depth)?;
            let visitor = Container {
                budget,
                depth,
                key_limit,
            };
            if first == b'{' {
                decoder.deserialize_map(visitor)
            } else {
                decoder.deserialize_seq(visitor)
            }
            .map_err(|_| invalid())?
        }
        b'"' => {
            if text.len() > MAX_STRING_BYTES * 6 + 2 {
                return Err(ValueError("JSON string length"));
            }
            Value::String(
                StringToken {
                    budget,
                    depth,
                    key: false,
                    maximum: MAX_STRING_BYTES,
                }
                .deserialize(&mut decoder)
                .map_err(|_| invalid())?,
            )
        }
        _ => {
            // Check integer bounds before converting the number into a core integer.
            let json: serde_json::Value =
                serde::Deserialize::deserialize(&mut decoder).map_err(|_| invalid())?;
            if let serde_json::Value::Number(number) = &json
                && !number.as_f64().is_some_and(f64::is_finite)
            {
                return Err(ValueError("JSON number out of range"));
            }
            let value = chigiri_http::value_from_json(json).map_err(|_| invalid())?;
            budget.node(&value, depth)?;
            if matches!(value, Value::Int(_) | Value::Float(_) | Value::BigInt(_)) {
                exact_number(&value)?;
            }
            value
        }
    };
    decoder.end().map_err(|_| invalid())?;
    Ok(value)
}

fn invalid() -> ValueError {
    ValueError("invalid bounded JSON value")
}

struct StringToken<'a> {
    budget: &'a mut ValueBudget,
    depth: usize,
    key: bool,
    maximum: usize,
}

impl<'de> DeserializeSeed<'de> for StringToken<'_> {
    type Value = String;
    fn deserialize<D: Deserializer<'de>>(self, decoder: D) -> Result<String, D::Error> {
        decoder.deserialize_str(self)
    }
}

impl Visitor<'_> for StringToken<'_> {
    type Value = String;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded UTF-8 string")
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<String, E> {
        if value.len() > self.maximum {
            return Err(E::custom("JSON string length"));
        }
        if self.key {
            self.budget.key_bytes(value.len())
        } else {
            self.budget.string_node(value.len(), self.depth)
        }
        .map_err(E::custom)?;
        // Copy the decoder's borrowed or temporary unescaped string only after reserving its budget.
        Ok(value.to_owned())
    }
}

struct Container<'a> {
    budget: &'a mut ValueBudget,
    depth: usize,
    key_limit: usize,
}

impl<'de> Visitor<'de> for Container<'_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded JSON container")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(raw) = sequence.next_element::<&RawValue>()? {
            if values.len() == MAX_ITEMS {
                return Err(A::Error::custom("JSON array limit"));
            }
            values.push(
                decode(raw, self.budget, self.depth + 1, self.key_limit)
                    .map_err(A::Error::custom)?,
            );
        }
        Ok(Value::List(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut mapping: A) -> Result<Value, A::Error> {
        let mut values = Object::new();
        while let Some(key) = mapping.next_key_seed(StringToken {
            budget: self.budget,
            depth: self.depth + 1,
            key: true,
            maximum: self.key_limit,
        })? {
            if values.contains_key(&key) {
                return Err(A::Error::custom("duplicate JSON key"));
            }
            let raw: &RawValue = mapping.next_value()?;
            values.insert(
                key,
                decode(raw, self.budget, self.depth + 1, self.key_limit)
                    .map_err(A::Error::custom)?,
            );
        }
        Ok(Value::Object(values))
    }
}
