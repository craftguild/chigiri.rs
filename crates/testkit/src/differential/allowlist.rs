//! Difference-allowlist parser requiring exact pointers and old/new values.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DifferenceClassification {
    SpecificationFix,
    IntendedProductChange,
    Regression,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllowlistEntry {
    pub case_id: String,
    pub pointer: String,
    pub old: Value,
    pub new: Value,
    pub reason: String,
    pub owner: String,
    pub classification: DifferenceClassification,
}

const MARKER_KEY: &str = "$chigiriDifference";
const MARKER_VALUES: [&str; 3] = ["missing", "object", "array"];

pub fn parse_allowlist(source: &str) -> Result<Vec<AllowlistEntry>, String> {
    let value: Value = serde_json::from_str(source)
        .map_err(|error| format!("cannot parse allowlist JSON: {error}"))?;
    let Value::Array(entries) = value else {
        return Err("the allowlist root must be an array".to_owned());
    };
    let parsed = entries
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let Value::Object(mut object) = value else {
                return Err(format!("The allowlist entry {index} must be an object."));
            };
            validate_keys(index, &object)?;
            let case_id = required_string(index, &mut object, "caseId")?;
            let pointer = required_string(index, &mut object, "pointer")?;
            let reason = required_string(index, &mut object, "reason")?;
            let owner = required_string(index, &mut object, "owner")?;
            let classification = parse_classification(
                index,
                &required_string(index, &mut object, "classification")?,
            )?;
            let old = object
                .remove("old")
                .ok_or_else(|| format!("allowlist entry {index} requires `old`"))?;
            let new = object
                .remove("new")
                .ok_or_else(|| format!("allowlist entry {index} requires `new`"))?;
            Ok(AllowlistEntry {
                case_id,
                pointer,
                old,
                new,
                reason,
                owner,
                classification,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_allowlist_entries(&parsed)?;
    Ok(parsed)
}

pub(super) fn validate_allowlist_entries(entries: &[AllowlistEntry]) -> Result<(), String> {
    let mut keys = BTreeSet::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.case_id.trim().is_empty() {
            return Err(format!(
                "allowlist entry {index} requires a nonempty string `caseId`"
            ));
        }
        if entry.reason.trim().is_empty() || entry.owner.trim().is_empty() {
            return Err(format!(
                "allowlist entry {index} requires nonempty `reason` and `owner` strings"
            ));
        }
        if !valid_difference_target(&entry.pointer, &entry.old, &entry.new) {
            return Err(format!(
                "pointer and `old`/`new` values for allowlist entry {index} do not satisfy the exact-difference contract"
            ));
        }
        if !keys.insert((entry.case_id.clone(), entry.pointer.clone())) {
            return Err(format!(
                "allowlist entry {index} duplicates a `caseId`/pointer pair"
            ));
        }
    }
    Ok(())
}

pub(super) fn difference_marker(kind: &str) -> Value {
    debug_assert!(MARKER_VALUES.contains(&kind));
    let mut marker = Map::new();
    marker.insert(MARKER_KEY.to_owned(), Value::String(kind.to_owned()));
    Value::Object(marker)
}

fn valid_expected_value(value: &Value) -> bool {
    match value {
        Value::Array(_) => false,
        Value::Object(_) => is_difference_marker(value),
        _ => true,
    }
}

fn is_difference_marker(value: &Value) -> bool {
    let Value::Object(object) = value else {
        return false;
    };
    object.len() == 1
        && object
            .get(MARKER_KEY)
            .and_then(Value::as_str)
            .is_some_and(|value| MARKER_VALUES.contains(&value))
}

fn valid_difference_target(pointer: &str, old: &Value, new: &Value) -> bool {
    if !valid_expected_value(old) || !valid_expected_value(new) {
        return false;
    }
    let Some(segments) = pointer_segments(pointer) else {
        return false;
    };
    match segments.as_slice() {
        [single] => single == "status" || single == "mediaType",
        [head, _, index] if head == "headers" => canonical_index(index),
        [head, member] if head == "body" => {
            matches!(member.as_str(), "data" | "errors" | "extensions")
                && (is_difference_marker(old) || is_difference_marker(new))
        }
        [head, _, _, rest @ ..] if head == "body" => !rest.iter().any(String::is_empty),
        _ => false,
    }
}

pub(super) fn pointer_segments(pointer: &str) -> Option<Vec<String>> {
    if !pointer.starts_with('/') || pointer.contains('*') || pointer.ends_with('/') {
        return None;
    }
    pointer[1..].split('/').map(decode_segment).collect()
}

pub(super) fn canonical_index(value: &str) -> bool {
    value == "0"
        || (!value.starts_with('0')
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && value.parse::<usize>().is_ok())
}

fn decode_segment(segment: &str) -> Option<String> {
    let mut decoded = String::new();
    let mut characters = segment.chars();
    while let Some(character) = characters.next() {
        if character != '~' {
            decoded.push(character);
            continue;
        }
        match characters.next()? {
            '0' => decoded.push('~'),
            '1' => decoded.push('/'),
            _ => return None,
        }
    }
    (!decoded.is_empty()).then_some(decoded)
}

fn validate_keys(index: usize, object: &Map<String, Value>) -> Result<(), String> {
    const KEYS: [&str; 7] = [
        "caseId",
        "pointer",
        "old",
        "new",
        "reason",
        "owner",
        "classification",
    ];
    for key in object.keys() {
        if !KEYS.contains(&key.as_str()) {
            return Err(format!("allowlist entry {index} has unknown key `{key}`"));
        }
    }
    if object.len() != KEYS.len() {
        return Err(format!("allowlist entry {index} requires every key"));
    }
    Ok(())
}

fn parse_classification(index: usize, value: &str) -> Result<DifferenceClassification, String> {
    match value {
        "specification-fix" => Ok(DifferenceClassification::SpecificationFix),
        "intended-product-change" => Ok(DifferenceClassification::IntendedProductChange),
        "regression" => Ok(DifferenceClassification::Regression),
        _ => Err(format!(
            "allowlist entry {index} has an unknown classification"
        )),
    }
}

fn required_string(
    index: usize,
    object: &mut Map<String, Value>,
    key: &str,
) -> Result<String, String> {
    let value = object
        .remove(key)
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("allowlist entry {index} requires a nonempty string `{key}`"))?;
    Ok(value)
}
