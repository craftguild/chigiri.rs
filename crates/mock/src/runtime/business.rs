//! Holds the first explicit resolver business rejection for one request.

use std::sync::{Arc, Mutex};

use chigiri::PathSegment;
use chigiri::core::{Object, Value};

use crate::value::ValueError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BusinessFailure {
    pub code: String,
    pub message: String,
    pub path: Arc<[PathSegment]>,
}

impl BusinessFailure {
    pub fn decode(object: &Object, path: Arc<[PathSegment]>) -> Result<Self, ValueError> {
        if object
            .iter()
            .any(|(name, _)| name != "code" && name != "message")
        {
            return Err(ValueError("business failure payload is not closed"));
        }
        let code = object
            .get("code")
            .and_then(Value::as_str)
            .filter(|code| valid_code(code))
            .ok_or(ValueError("invalid business failure code"))?;
        let message = match object.get("message") {
            None => "Request failed".to_owned(),
            Some(Value::String(message)) if message.len() <= crate::value::MAX_STRING_BYTES => {
                message.clone()
            }
            _ => return Err(ValueError("invalid business failure message")),
        };
        Ok(Self {
            code: code.to_owned(),
            message,
            path,
        })
    }
}

fn valid_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes
            .iter()
            .skip(1)
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
        && !code.starts_with("MOCK_")
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BusinessLedger(Arc<Mutex<Option<BusinessFailure>>>);

impl BusinessLedger {
    pub fn record(&self, failure: BusinessFailure) {
        let mut current = self.0.lock().expect("business failure ledger");
        if current.is_none() {
            *current = Some(failure);
        }
    }

    pub fn get(&self) -> Option<BusinessFailure> {
        self.0.lock().expect("business failure ledger").clone()
    }
}

#[cfg(test)]
mod tests {
    use super::valid_code;

    #[test]
    fn business_codes_are_closed_and_reserve_the_host_prefix() {
        for code in ["DENIED", "A1", "A_B", &"A".repeat(64)] {
            assert!(valid_code(code));
        }
        for code in ["", "lower", "1BAD", "BAD-DASH", "MOCK_PRIVATE"] {
            assert!(!valid_code(code));
        }
    }
}
