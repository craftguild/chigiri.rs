//! Read and write internal management metadata without exposing it as a public project input.

use std::fmt;
use std::io::{self, Write};

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::ProjectError;

pub(super) const MAX_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_FILES: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(super) struct Entries<T>(pub Vec<T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Entries<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Items<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Items<T> {
            type Value = Entries<T>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("bounded managed entries")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(raw) = sequence.next_element::<&serde_json::value::RawValue>()? {
                    if entries.len() == MAX_FILES {
                        return Err(serde::de::Error::custom("managed file count limit"));
                    }
                    entries
                        .push(serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?);
                }
                Ok(Entries(entries))
            }
        }
        deserializer.deserialize_seq(Items(std::marker::PhantomData))
    }
}

pub(super) fn facts<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<std::collections::BTreeMap<String, super::digest::Digest>, D::Error> {
    struct Facts;
    impl<'de> Visitor<'de> for Facts {
        type Value = std::collections::BTreeMap<String, super::digest::Digest>;
        fn expecting(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
            out.write_str("bounded unique schema facts")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut result = std::collections::BTreeMap::new();
            let mut bytes = 0usize;
            while let Some(key) = map.next_key::<String>()? {
                if result.len() == 16384
                    || !fact_key(&key)
                    || result.contains_key(&key)
                    || key.len() + 64 > MAX_BYTES - bytes
                {
                    return Err(serde::de::Error::custom("invalid schema fact inventory"));
                }
                bytes += key.len() + 64;
                result.insert(key, map.next_value()?);
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Facts)
}

fn fact_key(key: &str) -> bool {
    let mut parts = key.split('/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("type" | "model"), Some(ty), None, None) => name(ty),
        (Some("input"), Some(ty), Some(field), None) => name(ty) && name(field),
        (Some("field" | "constraint" | "relation"), Some(field), None, None) => coordinate(field),
        (Some("argument"), Some(field), Some(argument), None) => {
            coordinate(field) && name(argument)
        }
        _ => false,
    }
}

pub(super) fn coordinate(value: &str) -> bool {
    value
        .split_once('.')
        .is_some_and(|(parent, field)| name(parent) && name(field))
}

pub(super) fn name(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

struct Bounded {
    bytes: Vec<u8>,
    remaining: usize,
}

impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other("metadata byte limit"));
        }
        self.remaining -= bytes.len();
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn render(value: &impl Serialize, remaining: usize) -> Result<String, ProjectError> {
    let mut sink = Bounded {
        bytes: Vec::new(),
        remaining,
    };
    serde_json::to_writer_pretty(&mut sink, value).map_err(|_| limit())?;
    sink.write_all(b"\n").map_err(|_| limit())?;
    String::from_utf8(sink.bytes).map_err(|_| limit())
}

fn limit() -> ProjectError {
    ProjectError::new("limit", "generated metadata exceeds 16 MiB")
}
