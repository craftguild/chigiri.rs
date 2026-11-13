//! Serialize validated data in deterministic JSON order without constructing a second large DOM.

use chigiri::core::{Object, Value};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use std::io::{self, Write};

struct Bounded {
    bytes: Vec<u8>,
    maximum: usize,
}

impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("JSON byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn render(value: &impl Serialize, maximum: usize) -> Result<String, super::ValueError> {
    let mut sink = Bounded {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut sink, value)
        .map_err(|_| super::ValueError("JSON byte limit or number"))?;
    String::from_utf8(sink.bytes).map_err(|_| super::ValueError("invalid JSON UTF-8"))
}

pub(crate) struct BorrowedObject<'a>(pub &'a Object);
impl Serialize for BorrowedObject<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        object(self.0, serializer)
    }
}

pub(crate) struct Borrowed<'a>(pub &'a Value);
impl Serialize for Borrowed<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Null => serializer.serialize_none(),
            Value::Boolean(value) => serializer.serialize_bool(*value),
            Value::Int(value) => serializer.serialize_i128(*value),
            Value::Float(value) => serializer.serialize_f64(*value),
            Value::BigInt(value) => {
                let number: serde_json::Number =
                    value.parse().map_err(serde::ser::Error::custom)?;
                number.serialize(serializer)
            }
            Value::String(value) => serializer.serialize_str(value),
            Value::List(values) => {
                let mut sequence = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    sequence.serialize_element(&Borrowed(value))?;
                }
                sequence.end()
            }
            Value::Object(values) => object(values, serializer),
        }
    }
}

pub(crate) fn object<S: Serializer>(values: &Object, serializer: S) -> Result<S::Ok, S::Error> {
    let mut mapping = serializer.serialize_map(Some(values.len()))?;
    // Sort borrowed entries so serialization does not depend on the core object's map implementation.
    let ordered = values.iter().collect::<std::collections::BTreeMap<_, _>>();
    for (key, value) in ordered {
        mapping.serialize_entry(key, &Borrowed(value))?;
    }
    mapping.end()
}
