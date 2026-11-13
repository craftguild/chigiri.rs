#![forbid(unsafe_code)]
//! fuzz/regression, common harness, giving the same input to the production line.

use chigiri_core::language::{parse_executable_document, parse_type_system_document};
use chigiri_core::schema::compose_schema;
use chigiri_core::{ExecutionLimits, SourceId};
use chigiri_http::{RequestHeader, RequestParts, decode_envelope, value_from_json, value_to_json};
use chigiri_operation::PersistedOperationId;

/// A production-input harness shared by fixed replay and coverage-guided fuzzing.
pub type Harness = fn(&[u8]);

pub const TARGETS: [(&str, Harness); 6] = [
    ("parser", parser),
    ("sdl_composition", sdl_composition),
    ("http_decode", http_decode),
    ("json_conversion", json_conversion),
    ("persisted_id", persisted_id),
    ("config_parser", config_parser),
];

pub fn parser(data: &[u8]) {
    if let Ok(source) = std::str::from_utf8(data) {
        let _ = parse_executable_document(SourceId::new(1), source, &ExecutionLimits::production());
    }
}

pub fn sdl_composition(data: &[u8]) {
    if let Ok(source) = std::str::from_utf8(data) {
        let parsed =
            parse_type_system_document(SourceId::new(2), source, &ExecutionLimits::production());
        if let Some(document) = parsed.document {
            let _ = compose_schema(&[document], &ExecutionLimits::production());
        }
    }
}

pub fn http_decode(data: &[u8]) {
    const HEADERS: &[RequestHeader<'_>] = &[
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"application/graphql-response+json"),
    ];
    let _ = decode_envelope(RequestParts {
        method: "POST",
        uri_query: None,
        headers: HEADERS,
        body: data,
    });
}

pub fn json_conversion(data: &[u8]) {
    if let Ok(value) = serde_json::from_slice(data) {
        if let Ok(value) = value_from_json(value) {
            let _ = value_to_json(&value);
        }
    }
}

pub fn persisted_id(data: &[u8]) {
    if let Ok(value) = std::str::from_utf8(data) {
        let _ = PersistedOperationId::new(value);
    }
}

pub fn config_parser(data: &[u8]) {
    if let Ok(source) = std::str::from_utf8(data) {
        chigiri_codegen::fuzz_parse_manifest(source);
    }
}

#[cfg(test)]
mod tests;
