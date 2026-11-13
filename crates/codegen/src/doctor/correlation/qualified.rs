//! Correlates spanless rustc diagnostics only with selected external trait implementations.

use serde_json::Value;

use super::{Sources, number};
use crate::DeveloperObject;

pub(super) fn root_trait(span: &Value, object: &DeveloperObject, sources: &Sources) -> bool {
    let Some(source) = span
        .get("file_name")
        .and_then(Value::as_str)
        .and_then(|path| sources.read(path))
    else {
        return false;
    };
    if source.contains("extern crate self") {
        return false;
    }
    let Some(implementation) = source.get(number(span, "byte_start")..number(span, "byte_end"))
    else {
        return false;
    };
    let Some(tail) = implementation.trim_start().strip_prefix("impl") else {
        return false;
    };
    if !tail.starts_with(char::is_whitespace) {
        return false;
    }
    let tail = tail.trim_start();
    [
        format!("::{}::{}", sources.alias, object.resolver_trait()),
        format!("::r#{}::{}", sources.alias, object.resolver_trait()),
    ]
    .into_iter()
    .any(|prefix| {
        tail.strip_prefix(&prefix)
            .is_some_and(|tail| tail.starts_with('<') || tail.starts_with(char::is_whitespace))
    })
}

pub(super) fn missing(message: &Value, method: &str) -> bool {
    message
        .get("message")
        .and_then(Value::as_str)
        .is_some_and(|message| {
            message.contains(&format!("`{method}`"))
                || message.contains(&format!("`{}`", method.trim_start_matches("r#")))
        })
}

/// Correlate a return-signature diagnostic only with a directly qualified trait
/// implementation. Ambiguous bodies, comments, and other syntax keep rustc's
/// original diagnostic without a guessed supplement.
pub(super) fn method(
    span: &Value,
    object: &DeveloperObject,
    method: &str,
    sources: &Sources,
) -> bool {
    let Some(source) = span
        .get("file_name")
        .and_then(Value::as_str)
        .and_then(|path| sources.read(path))
    else {
        return false;
    };
    let end = number(span, "byte_end");
    let Some(prefix) = source.get(..end) else {
        return false;
    };
    let token = format!("fn {method}");
    let Some(method_start) = prefix.rfind(&token) else {
        return false;
    };
    let Some(signature) = source.get(method_start + token.len()..end) else {
        return false;
    };
    if !(signature.starts_with('<') || signature.starts_with('('))
        || signature.contains(['{', '}', ';', '/', '"'])
    {
        return false;
    }
    let Some(body) = super::scope::enclosing_brace(&source, method_start) else {
        return false;
    };
    let Some(implementation) = source[..body].rfind("impl ") else {
        return false;
    };
    if source[implementation..body].contains(['{', '}', ';', '/', '"']) {
        return false;
    }
    let mut implementation_span = span.clone();
    implementation_span["byte_start"] = implementation.into();
    implementation_span["byte_end"] = body.into();
    root_trait(&implementation_span, object, sources)
}
