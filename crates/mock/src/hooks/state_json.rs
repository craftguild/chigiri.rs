//! Decode state configuration without reevaluating Luau source.

use serde::Deserialize;
use serde_json::value::RawValue;

use super::StateContract;
use crate::value::{MAX_BYTES, MAX_STRING_BYTES, ValueBudget, ValueError};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawState<'a> {
    #[serde(borrow)]
    fields: &'a RawValue,
    #[serde(borrow)]
    initial: &'a RawValue,
}

impl StateContract {
    /// Configuration value only; it is not live request state.
    pub fn to_json(&self, maximum: usize) -> Result<String, ValueError> {
        crate::value::serialize::render(self, maximum.min(MAX_BYTES))
    }

    /// Validate type declarations and initial values decoded from acyclic JSON.
    pub fn from_json(source: &str, budget: &mut ValueBudget) -> Result<Self, ValueError> {
        if source.len() > MAX_BYTES {
            return Err(ValueError("state JSON byte limit"));
        }
        let raw: RawState<'_> =
            serde_json::from_str(source).map_err(|_| ValueError("invalid state JSON envelope"))?;
        budget.container(1)?;
        budget.key("fields")?;
        budget.key("initial")?;
        let fields = crate::value::json::decode(raw.fields, budget, 1, MAX_STRING_BYTES)?;
        let initial = crate::value::json::decode(raw.initial, budget, 1, MAX_STRING_BYTES)?;
        Self::parse(&fields, &initial, budget)
    }
}
