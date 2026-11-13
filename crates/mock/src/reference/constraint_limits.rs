//! Publish regex build and execution limits from their authoritative constants.

use super::{
    ReferenceError,
    api_types::{LimitDoc, LimitUnit},
    budget::Budget,
};
use crate::api::constraints::*;

pub(super) fn append(rows: &mut Vec<LimitDoc>, budget: &mut Budget) -> Result<(), ReferenceError> {
    use LimitUnit::*;
    for (id, value, unit, scope, description) in [
        (
            "regex.source_bytes",
            REGEX_SOURCE_BYTES,
            Bytes,
            "pattern",
            "Maximum UTF-8 bytes in one regex source",
        ),
        (
            "regex.patterns",
            REGEX_PATTERNS,
            Items,
            "schema",
            "Maximum distinct regex patterns in one schema",
        ),
        (
            "regex.schema_source_bytes",
            REGEX_SOURCE_BYTES * REGEX_PATTERNS,
            Bytes,
            "schema",
            "Maximum aggregate interned regex-source bytes",
        ),
        (
            "regex.compiled_bytes",
            REGEX_COMPILED_BYTES,
            Bytes,
            "pattern",
            "Maximum compiled bytes reserved for one pattern",
        ),
        (
            "regex.cache_bytes",
            REGEX_CACHE_BYTES,
            Bytes,
            "pattern",
            "Maximum DFA cache bytes reserved for one pattern",
        ),
        (
            "regex.schema_compiled_bytes",
            REGEX_COMPILED_BYTES * REGEX_PATTERNS,
            Bytes,
            "schema",
            "Aggregate compiled-byte reservation made before schema construction",
        ),
        (
            "regex.schema_cache_bytes",
            REGEX_CACHE_BYTES * REGEX_PATTERNS,
            Bytes,
            "schema",
            "Aggregate DFA cache-byte reservation for one schema",
        ),
        (
            "regex.nesting",
            REGEX_NESTING as usize,
            Depth,
            "pattern",
            "Regex parser nesting limit",
        ),
        (
            "regex.build_time",
            REGEX_BUILD_SECONDS as usize * 1000,
            Milliseconds,
            "schema",
            "Shared deadline for regex validation and construction",
        ),
        (
            "regex.matches",
            REGEX_MATCHES,
            Calls,
            "scope",
            "Regex-match calls shared by one startup validation or request",
        ),
        (
            "regex.match_bytes",
            REGEX_MATCH_BYTES,
            Bytes,
            "scope",
            "Aggregate matched input bytes, including misses and repeated checks",
        ),
        (
            "regex.input_bytes",
            crate::value::MAX_STRING_BYTES,
            Bytes,
            "match",
            "Maximum UTF-8 bytes in one matched value",
        ),
        (
            "regex.work_factor",
            REGEX_WORK_FACTOR,
            Work,
            "match",
            "Work reservation coefficient: 1 + factor × (input bytes + 1)",
        ),
    ] {
        budget.push(
            rows,
            LimitDoc {
                id: id.into(),
                value,
                unit,
                shared_scope: scope.into(),
                description: description.into(),
            },
        )?;
    }
    Ok(())
}
