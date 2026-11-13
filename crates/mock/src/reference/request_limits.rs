//! Publish request soft and hard limits from the runtime policy.

use super::{
    ReferenceError,
    api_types::{LimitDoc, LimitUnit},
    budget::Budget,
};
use crate::limits::request::{KINDS, LimitKind, policy};

pub(super) fn append(rows: &mut Vec<LimitDoc>, budget: &mut Budget) -> Result<(), ReferenceError> {
    for kind in KINDS {
        let policy = policy(kind);
        for (boundary, value, description) in [
            (
                "soft",
                policy.soft,
                "Emit one warning for this resource in a request, then continue processing.",
            ),
            (
                "hard",
                policy.hard,
                if kind == LimitKind::ElapsedMs {
                    "Reject the candidate when the absolute deadline is reached."
                } else {
                    "Allow only reserved overshoot and reject the entire candidate before commit."
                },
            ),
        ] {
            let unit = match kind {
                LimitKind::Work => LimitUnit::Work,
                LimitKind::ValueNodes => LimitUnit::Nodes,
                LimitKind::LogicalBytes | LimitKind::RegexInputBytes => LimitUnit::Bytes,
                LimitKind::RegexMatches => LimitUnit::Calls,
                LimitKind::ElapsedMs => LimitUnit::Milliseconds,
            };
            budget.push(
                rows,
                LimitDoc {
                    id: format!("request.{}.{boundary}", policy.name),
                    value,
                    unit,
                    shared_scope: "request".into(),
                    description: description.into(),
                },
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishes_all_12_boundaries_from_the_shared_policy() {
        let document = super::super::ReferenceDocument::build(None).unwrap();
        let bytes = document.render(super::super::Format::Json).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let limits = json["limits"].as_array().unwrap();
        for kind in KINDS {
            let policy = policy(kind);
            for (boundary, value) in [("soft", policy.soft), ("hard", policy.hard)] {
                let id = format!("request.{}.{boundary}", policy.name);
                let rows: Vec<_> = limits.iter().filter(|row| row["id"] == id).collect();
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0]["value"], value);
                assert_eq!(rows[0]["shared_scope"], "request");
            }
        }
    }
}
