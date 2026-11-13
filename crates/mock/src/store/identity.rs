//! Creates runtime-generated ULIDs and canonical index keys for validated
//! identities.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chigiri::core::Value;

use super::StoreError;
use crate::value::ValueError;

pub(super) fn generate() -> Result<String, StoreError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ValueError("ULID clock unavailable"))?
        .as_millis();
    if millis >= (1u128 << 48) {
        return Err(ValueError("ULID timestamp range"));
    }
    let mut random = [0u8; 16];
    getrandom::fill(&mut random[6..]).map_err(|_| ValueError("ULID entropy unavailable"))?;
    Ok(
        ulid::Ulid::from_parts(millis as u64, u128::from_be_bytes(random))
            .to_string()
            .to_ascii_lowercase(),
    )
}

pub(super) fn value(value: Option<&Value>) -> Result<Option<Arc<str>>, StoreError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(Arc::from(value.as_str()))),
        Some(Value::Int(value)) => Ok(Some(Arc::from(value.to_string()))),
        Some(Value::Boolean(value)) => Ok(Some(Arc::from(if *value { "true" } else { "false" }))),
        _ => Err(ValueError("invalid identity value")),
    }
}
