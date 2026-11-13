//! Preserve ADR-0039's HTTP 200 GraphQL envelope without inventing HTTP status or source diagnostics.

use chigiri::core::{Object, Value};

/// First valid rejection produced by one hook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    /// Validated HTTP status.
    pub status: u16,
    /// Public message, or a fixed safe fallback for invalid input.
    pub message: String,
    /// Optional diagnostic for an invalid rejection declaration.
    pub invalid: bool,
}

impl Rejection {
    pub(crate) fn invalid() -> Self {
        Self {
            status: 500,
            message: "Internal server error".into(),
            invalid: true,
        }
    }

    pub(crate) fn decode(object: &Object) -> Self {
        let status = match object.get("code") {
            Some(Value::Int(value)) => u16::try_from(*value).ok(),
            Some(Value::Float(value)) if value.is_finite() && value.fract() == 0.0 => {
                (200.0..=511.0).contains(value).then_some(*value as u16)
            }
            _ => None,
        };
        let Some(status) = status.filter(|status| assigned(*status)) else {
            return Self::invalid();
        };
        if object
            .iter()
            .any(|(key, _)| key != "code" && key != "message")
        {
            return Self::invalid();
        }
        let message = match object.get("message") {
            None => "Request rejected".to_owned(),
            Some(Value::String(message)) if message.len() <= crate::value::MAX_STRING_BYTES => {
                message.clone()
            }
            _ => return Self::invalid(),
        };
        Self {
            status,
            message,
            invalid: false,
        }
    }
}

/// Fixed status set as of 2026-09-09; no live registry lookup occurs at startup.
pub const fn assigned(status: u16) -> bool {
    matches!(status, 200 | 400..=417
        | 421..=426 | 428 | 429 | 431 | 451 | 500..=508 | 510 | 511)
}
