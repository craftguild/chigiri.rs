//! Adds contextual diagnostics only when rustc output, spans, generated symbols,
//! and schema hints are all available.

mod chain;
mod output;
mod qualified;
mod scope;

use crate::{CodegenDiagnostic, DeveloperSchema, DiagnosticLabel};
use serde_json::Value;

use super::source::Sources;

pub(super) fn supplements(
    record: &Value,
    schema: &DeveloperSchema,
    root: &Sources,
) -> Vec<CodegenDiagnostic> {
    let Some(message) = record.get("message") else {
        return Vec::new();
    };
    let code = message
        .pointer("/code/code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !has_application_primary(message, root) {
        return Vec::new();
    }
    match code {
        "E0599" => missing_stage(message, schema, root).into_iter().collect(),
        "E0046" => missing_trait_fields(message, schema, root),
        "E0053" | "E0271" | "E0308" => wrong_output(message, schema, root).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn missing_stage(
    message: &Value,
    schema: &DeveloperSchema,
    root: &Sources,
) -> Option<CodegenDiagnostic> {
    let text = message.get("message")?.as_str()?;
    let primary = primary_span(message)?;
    if text.contains("ResolverStage<") && chain::missing(primary, root, "Resolvers") {
        let index = stage_index(text, "ResolverStage")?;
        let object = schema.objects().get(index)?;
        return Some(
            base_hint(
                "SF4018",
                format!(
                    "Resolver group `{}` is not registered.",
                    object.graphql_name()
                ),
                primary,
                root,
            )
            .with_help(format!(
                "Add `.{}(...)` to the builder next.",
                object.assembly_method()
            )),
        );
    }
    None
}

fn missing_trait_fields(
    message: &Value,
    schema: &DeveloperSchema,
    root: &Sources,
) -> Vec<CodegenDiagnostic> {
    let Some(primary) = primary_span(message) else {
        return Vec::new();
    };
    let source = span_text(primary, root);
    let Some(object) = schema.objects().iter().find(|object| {
        source.contains(&format!("::{}", object.resolver_trait()))
            && (has_generated_span(message, root) || qualified::root_trait(primary, object, root))
    }) else {
        return Vec::new();
    };
    object
        .fields()
        .iter()
        .filter(|field| {
            generated_field_span(message, field.field_name(), root)
                || (qualified::root_trait(primary, object, root)
                    && qualified::missing(message, field.field_name()))
        })
        .map(|field| {
            base_hint(
                "SF4018",
                format!(
                    "There is no field implementation of `{}`.",
                    field.coordinate()
                ),
                primary,
                root,
            )
            .with_coordinate(field.coordinate())
            .with_help(format!(
                "Implement the `{}` method on the `{}` trait.",
                field.field_name(),
                object.resolver_trait()
            ))
        })
        .take(1000)
        .collect()
}

fn wrong_output(
    message: &Value,
    schema: &DeveloperSchema,
    root: &Sources,
) -> Option<CodegenDiagnostic> {
    if !output::mismatch(message) {
        return None;
    }
    let primary = primary_span(message)?;
    let (_object, field) = schema.objects().iter().find_map(|object| {
        object.fields().iter().find_map(|field| {
            (qualified::method(primary, object, field.field_name(), root)
                && (generated_field_span(message, field.field_name(), root)
                    || spans(message).any(|span| {
                        is_generated(span, root)
                            && span
                                .get("label")
                                .and_then(Value::as_str)
                                .is_some_and(|label| {
                                    label.contains(&format!("{}::", object.resolver_trait()))
                                })
                    })))
            .then_some((object, field))
        })
    })?;
    Some(
        base_hint(
            "SF4019",
            format!(
                "The output type of `{}` does not match.",
                field.coordinate()
            ),
            primary,
            root,
        )
        .with_coordinate(field.coordinate())
        .with_help(format!(
            "Return Rust type `{}` compatible with GraphQL type `{}` from `{}`.",
            field.rust_output(),
            field.graphql_output(),
            field.field_name()
        )),
    )
}

fn stage_index(message: &str, symbol: &str) -> Option<usize> {
    let start = message.find(&format!("{symbol}<"))?;
    let tail = message.get(start + symbol.len()..)?;
    let mut depth = 0usize;
    let mut last_comma = None;
    for (index, character) in tail.char_indices() {
        match character {
            // Ambiguous const expressions/characters keep the original rustc
            // message without guessing a registration position.
            '{' | '}' | '\'' | '"' => return None,
            '<' => depth += 1,
            '>' if !tail[..index].ends_with('-') => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return tail.get(last_comma?..index)?.trim().parse().ok();
                }
            }
            ',' if depth == 1 => last_comma = Some(index + 1),
            _ => {}
        }
    }
    None
}

fn generated_field_span(message: &Value, field: &str, root: &Sources) -> bool {
    spans(message).any(|span| {
        is_generated(span, root)
            && (span
                .get("label")
                .and_then(Value::as_str)
                .is_some_and(|label| label.contains(&format!("`{field}` from trait")))
                || span_text(span, root).contains(&format!("fn {field}")))
    })
}

fn has_application_primary(message: &Value, root: &Sources) -> bool {
    primary_span(message).is_some_and(|span| !is_generated(span, root))
}

fn has_generated_span(message: &Value, root: &Sources) -> bool {
    spans(message).any(|span| is_generated(span, root))
}

fn is_generated(span: &Value, root: &Sources) -> bool {
    span.get("file_name")
        .and_then(Value::as_str)
        .is_some_and(|path| root.generated(path))
}

fn primary_span(message: &Value) -> Option<&Value> {
    spans(message).find(|span| span.get("is_primary").and_then(Value::as_bool) == Some(true))
}

fn spans(message: &Value) -> impl Iterator<Item = &Value> {
    // rustc puts future-output bounds in child notes on both supported lanes.
    // Use the same depth bound as diagnostic conversion and preserve source order.
    let mut pending = vec![(message, 0)];
    let mut result = Vec::new();
    while let Some((message, depth)) = pending.pop() {
        if depth > 64 {
            return Vec::new().into_iter();
        }
        result.extend(
            message
                .get("spans")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        );
        pending.extend(
            message
                .get("children")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .rev()
                .map(|child| (child, depth + 1)),
        );
    }
    result.into_iter()
}

fn span_text(span: &Value, root: &Sources) -> String {
    let source = span
        .get("file_name")
        .and_then(Value::as_str)
        .and_then(|path| root.read(path));
    source
        .and_then(|source| {
            source
                .lines()
                .nth(number(span, "line_start").saturating_sub(1))
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

fn base_hint(
    code: &'static str,
    message: String,
    span: &Value,
    root: &Sources,
) -> CodegenDiagnostic {
    let path = span
        .get("file_name")
        .and_then(Value::as_str)
        .map(|path| root.label(path))
        .unwrap_or_else(|| "<source>".to_owned());
    let start = number(span, "byte_start");
    let end = number(span, "byte_end").max(start);
    let line = number(span, "line_start").max(1);
    let column = number(span, "column_start").max(1);
    CodegenDiagnostic::error(code, message).with_primary(DiagnosticLabel::new(
        path,
        start..end,
        line,
        column,
        "consumer source",
    ))
}

fn number(value: &Value, field: &str) -> usize {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_real;
