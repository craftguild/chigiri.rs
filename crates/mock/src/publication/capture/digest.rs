//! Stable schema-bound digest for captured Record provenance.

use chigiri::core::SchemaDigest;
use sha2::{Digest as _, Sha256};

use super::{CapturedField, CapturedValue};
use crate::publication::meter::DIGEST_BYTES;

pub(super) fn record_digest(
    schema: SchemaDigest,
    type_name: &str,
    model: &str,
    identity: Option<&str>,
    fields: &[CapturedField],
) -> [u8; DIGEST_BYTES] {
    let mut digest = Sha256::new();
    digest.update(b"chigiri.mock.publication.record.v1\0");
    digest.update(schema.as_bytes());
    hash_text(&mut digest, type_name);
    hash_text(&mut digest, model);
    match identity {
        Some(identity) => {
            digest.update([1]);
            hash_text(&mut digest, identity);
        }
        None => digest.update([0]),
    }
    hash_fields(&mut digest, fields);
    digest.finalize().into()
}

fn hash_fields(digest: &mut Sha256, fields: &[CapturedField]) {
    digest.update((fields.len() as u64).to_le_bytes());
    for field in fields {
        hash_text(digest, &field.name);
        hash_value(digest, &field.value);
    }
}

fn hash_text(digest: &mut Sha256, value: &str) {
    digest.update((value.len() as u64).to_le_bytes());
    digest.update(value.as_bytes());
}

fn hash_value(digest: &mut Sha256, value: &CapturedValue) {
    let tag = match value {
        CapturedValue::Null => 0,
        CapturedValue::Boolean(false) => 1,
        CapturedValue::Boolean(true) => 2,
        CapturedValue::Integer(_) => 3,
        CapturedValue::Float(_) => 4,
        CapturedValue::BigInt(_) => 5,
        CapturedValue::String(_) => 6,
        CapturedValue::List(_) => 7,
        CapturedValue::Map(_) => 8,
        CapturedValue::Object(_) => 9,
    };
    digest.update([tag]);
    match value {
        CapturedValue::Null | CapturedValue::Boolean(_) => {}
        CapturedValue::Integer(value) => digest.update(value.to_le_bytes()),
        CapturedValue::Float(value) => digest.update(value.to_le_bytes()),
        CapturedValue::BigInt(value) | CapturedValue::String(value) => hash_text(digest, value),
        CapturedValue::List(values) => {
            digest.update((values.len() as u64).to_le_bytes());
            for value in values {
                hash_value(digest, value);
            }
        }
        CapturedValue::Map(fields) => hash_fields(digest, fields),
        CapturedValue::Object(object) => {
            hash_text(digest, &object.type_name);
            hash_fields(digest, &object.fields);
        }
    }
}
