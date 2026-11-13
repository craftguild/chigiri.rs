//! Scan complete JSONL capacity independently from line-level semantic validation.

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};

use super::SeedError;
use crate::ir::{MockIr, ModelId};
use crate::value::{MAX_ITEMS, MAX_STRING_BYTES, ValueBudget, ValueError};

/// This capacity scan does not replace decoding or store validation.
/// It deliberately ignores syntax errors so later lines are still charged; the full decoder
/// separately reports syntax, duplicate-key, and storage-contract failures.
pub(super) fn check(ir: &MockIr, files: &[(ModelId, &str)]) -> Result<(), SeedError> {
    let mut bytes = 0usize;
    let mut records = 0usize;
    let mut seen = BTreeSet::new();
    let mut budget = ValueBudget::default();
    budget.container(0).map_err(|e| SeedError::new(e.0))?;
    for &(model, source) in files {
        let error = |reason| SeedError::new(reason).at(model, 1);
        let definition = ir
            .models()
            .get(model.0)
            .ok_or_else(|| error("unknown seed model"))?;
        if !seen.insert(model) {
            return Err(error("duplicate seed model"));
        }
        if source.len() > crate::limits::SEED_BYTES.saturating_sub(bytes) {
            return Err(error("aggregate JSONL byte limit"));
        }
        bytes += source.len();
        let name = &ir.graph().types()[definition.ty.0].name;
        budget
            .key(name)
            .and_then(|_| budget.container(0))
            .map_err(|e| error(e.0))?;
        for (index, line) in source.split_terminator('\n').enumerate() {
            let error = |reason| SeedError::new(reason).at(model, index + 1);
            nesting(line).map_err(|()| error("seed value depth limit"))?;
            if index == super::MODEL_RECORDS || records == super::TOTAL_RECORDS {
                return Err(error("seed record capacity"));
            }
            records += 1;
            let mut exceeded = false;
            let mut decoder = serde_json::Deserializer::from_str(line);
            let _syntax = Scan {
                budget: &mut budget,
                exceeded: &mut exceeded,
                depth: 1,
            }
            .deserialize(&mut decoder);
            if exceeded {
                return Err(error("seed value resource limit"));
            }
        }
    }
    if seen.len() != ir.models().len() {
        return Err(SeedError::new("active seed coverage mismatch"));
    }
    Ok(())
}

/// Enforce depth before `RawValue`'s recursion guard by scanning without constructing a DOM.
fn nesting(line: &str) -> Result<(), ()> {
    let mut depth = 0usize;
    let mut string = false;
    let mut escaped = false;
    for byte in line.bytes() {
        if string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string = false;
            }
        } else {
            match byte {
                b'"' => string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > crate::value::MAX_DEPTH {
                        return Err(());
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}

struct Scan<'a> {
    budget: &'a mut ValueBudget,
    exceeded: &'a mut bool,
    depth: usize,
}

impl Scan<'_> {
    fn charge<E: Error>(&mut self, result: Result<(), ValueError>) -> Result<(), E> {
        result.map_err(|_| {
            *self.exceeded = true;
            E::custom("seed value resource limit")
        })
    }
    fn primitive<E: Error>(mut self, bytes: usize) -> Result<(), E> {
        let result = self.budget.string_node(bytes, self.depth);
        self.charge(result)
    }
}

impl<'de> DeserializeSeed<'de> for Scan<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(mut self, decoder: D) -> Result<(), D::Error> {
        use serde::Deserialize;
        let raw = <&serde_json::value::RawValue>::deserialize(decoder)?;
        let text = raw.get();
        if matches!(text.as_bytes()[0], b'-' | b'0'..=b'9') {
            // The implementation-only arbitrary-precision map is not a logical value node.
            let json = serde_json::from_str(text).map_err(D::Error::custom)?;
            if let Ok(value) = chigiri_http::value_from_json(json) {
                let result = self.budget.node(&value, self.depth);
                return self.charge(result);
            }
            return self.primitive(8);
        }
        let mut nested = serde_json::Deserializer::from_str(text);
        serde::Deserializer::deserialize_any(&mut nested, self).map_err(D::Error::custom)
    }
}

impl<'de> Visitor<'de> for Scan<'_> {
    type Value = ();
    fn expecting(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str("bounded seed value")
    }
    fn visit_unit<E: Error>(self) -> Result<(), E> {
        self.primitive(1)
    }
    fn visit_bool<E: Error>(self, _: bool) -> Result<(), E> {
        self.primitive(1)
    }
    fn visit_i64<E: Error>(self, _: i64) -> Result<(), E> {
        self.primitive(8)
    }
    fn visit_u64<E: Error>(self, _: u64) -> Result<(), E> {
        self.primitive(8)
    }
    fn visit_f64<E: Error>(self, _: f64) -> Result<(), E> {
        self.primitive(8)
    }
    fn visit_str<E: Error>(mut self, text: &str) -> Result<(), E> {
        if text.len() > MAX_STRING_BYTES {
            return self.charge(Err(ValueError("seed string length")));
        }
        self.primitive(text.len())
    }
    fn visit_seq<A: SeqAccess<'de>>(mut self, mut sequence: A) -> Result<(), A::Error> {
        let result = self.budget.container(self.depth);
        self.charge(result)?;
        let mut count = 0usize;
        while sequence
            .next_element_seed(Scan {
                budget: self.budget,
                exceeded: self.exceeded,
                depth: self.depth + 1,
            })?
            .is_some()
        {
            count += 1;
            if count > MAX_ITEMS {
                return self.charge(Err(ValueError("seed array limit")));
            }
        }
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(mut self, mut mapping: A) -> Result<(), A::Error> {
        let result = self.budget.container(self.depth);
        self.charge(result)?;
        while let Some(key) = mapping.next_key::<std::borrow::Cow<'de, str>>()? {
            let result = self.budget.key(&key);
            self.charge(result)?;
            mapping.next_value_seed(Scan {
                budget: self.budget,
                exceeded: self.exceeded,
                depth: self.depth + 1,
            })?;
        }
        Ok(())
    }
}
