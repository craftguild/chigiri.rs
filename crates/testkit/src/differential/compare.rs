//! Recursively compares response structures, consuming only exact allowlist entries.

mod value;

use std::collections::BTreeSet;
use std::fmt;

use super::allowlist::{canonical_index, pointer_segments, validate_allowlist_entries};
use super::{AllowlistEntry, DifferenceClassification, ObservedResponse};
use value::compare_case;

pub struct ComparisonCase<'a> {
    pub case_id: &'a str,
    pub old: &'a ObservedResponse,
    pub new: &'a ObservedResponse,
    pub header_names: &'a [String],
    pub volatile_extension_pointers: &'a [String],
}

#[derive(Debug)]
pub struct ComparisonError {
    message: String,
}

impl fmt::Display for ComparisonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ComparisonError {}

#[derive(Debug)]
pub(super) struct Difference {
    pub(super) case_id: String,
    pub(super) pointer: String,
    pub(super) old: serde_json::Value,
    pub(super) new: serde_json::Value,
}

pub fn compare_corpus(
    cases: &[ComparisonCase<'_>],
    allowlist: &[AllowlistEntry],
) -> Result<(), ComparisonError> {
    validate_policy(cases, allowlist)?;
    let mut differences = Vec::new();
    let mut consumed_volatile = BTreeSet::new();
    for case in cases {
        compare_case(case, &mut differences, &mut consumed_volatile);
    }
    for case in cases {
        if let Some(pointer) = case.volatile_extension_pointers.iter().find(|pointer| {
            !consumed_volatile.contains(&(case.case_id.to_owned(), (*pointer).clone()))
        }) {
            return Err(comparison_error(format!(
                "unused volatile extension pointer: case `{}` pointer {}",
                case.case_id, pointer
            )));
        }
    }
    let mut consumed = BTreeSet::new();
    for difference in differences {
        let matched = allowlist.iter().enumerate().find(|(index, entry)| {
            !consumed.contains(index)
                && entry.case_id == difference.case_id
                && entry.pointer == difference.pointer
                && entry.old == difference.old
                && entry.new == difference.new
        });
        let Some((index, entry)) = matched else {
            return Err(ComparisonError {
                message: format!(
                    "case `{}` has unallowlisted diff at {}: old={}, new={}",
                    difference.case_id, difference.pointer, difference.old, difference.new
                ),
            });
        };
        debug_assert_ne!(entry.classification, DifferenceClassification::Regression);
        consumed.insert(index);
    }
    if let Some((index, entry)) = allowlist
        .iter()
        .enumerate()
        .find(|(index, _)| !consumed.contains(index))
    {
        return Err(ComparisonError {
            message: format!(
                "unused allowlist entry {index}: case `{}` pointer {}",
                entry.case_id, entry.pointer
            ),
        });
    }
    Ok(())
}

fn validate_policy(
    cases: &[ComparisonCase<'_>],
    allowlist: &[AllowlistEntry],
) -> Result<(), ComparisonError> {
    validate_allowlist_entries(allowlist).map_err(comparison_error)?;
    let mut case_ids = BTreeSet::new();
    for case in cases {
        if case.case_id.trim().is_empty() || !case_ids.insert(case.case_id) {
            return Err(comparison_error(format!(
                "duplicate or empty comparison case ID: `{}`",
                case.case_id
            )));
        }
        validate_volatile_policy(case)?;
    }
    for (index, entry) in allowlist.iter().enumerate() {
        if !case_ids.contains(entry.case_id.as_str()) {
            return Err(comparison_error(format!(
                "allowlist entry {index} refers to unknown case `{}`",
                entry.case_id
            )));
        }
        if entry.classification == DifferenceClassification::Regression {
            return Err(comparison_error(format!(
                "allowlist entry {index} uses regression classification"
            )));
        }
    }
    Ok(())
}

fn validate_volatile_policy(case: &ComparisonCase<'_>) -> Result<(), ComparisonError> {
    let mut unique = BTreeSet::new();
    for pointer in case.volatile_extension_pointers {
        let valid = pointer_segments(pointer).is_some_and(|segments| {
            segments.len() >= 5
                && segments[0] == "body"
                && segments[1] == "errors"
                && canonical_index(&segments[2])
                && segments[3] == "extensions"
        });
        if !valid || !unique.insert(pointer) {
            return Err(comparison_error(format!(
                "case `{}` has invalid or duplicate volatile extension pointer {}",
                case.case_id, pointer
            )));
        }
    }
    Ok(())
}

fn comparison_error(message: String) -> ComparisonError {
    ComparisonError { message }
}
