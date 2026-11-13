//! Decode rustc's future-output comparison, without inferring types from application source.

use serde_json::Value;

pub(super) fn mismatch(message: &Value) -> bool {
    let Some(text) = message.get("message").and_then(Value::as_str) else {
        return false;
    };
    let Some((_, comparison)) = text.split_once("to be a future that resolves to `") else {
        return false;
    };
    let Some((expected, actual)) = comparison.split_once("`, but it resolves to `") else {
        return false;
    };
    let Some(actual) = actual.strip_suffix('`') else {
        return false;
    };
    match (success(expected), success(actual)) {
        (Some(expected), Some(actual)) => expected != actual,
        _ => false,
    }
}

/// Only split the first argument of rustc's normalized `Result` rendering.
/// Unknown formats, elided types, or ambiguous delimiters retain rustc alone.
fn success(result: &str) -> Option<&str> {
    let inner = result.strip_prefix("Result<")?.strip_suffix('>')?;
    let mut delimiters = Vec::new();
    for (index, character) in inner.char_indices() {
        match character {
            '<' | '(' | '[' => {
                if delimiters.len() == 64 {
                    return None;
                }
                delimiters.push(character);
            }
            '>' if inner[..index].ends_with('-') => {}
            '>' | ')' | ']' => {
                let open = match character {
                    '>' => '<',
                    ')' => '(',
                    _ => '[',
                };
                if delimiters.pop()? != open {
                    return None;
                }
            }
            ',' if delimiters.is_empty() => {
                let output = inner[..index].trim();
                let unknown = output
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .any(|token| token == "_");
                return (!output.is_empty()
                    && !inner[index + 1..].trim().is_empty()
                    && !unknown
                    && !output.contains("..."))
                .then_some(output);
            }
            '{' | '}' | '\'' | '"' => return None,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{mismatch, success};
    use serde_json::json;

    #[test]
    fn successful_result_position_is_separate_from_the_error() {
        for (expected, actual, changed) in [
            ("Result<String, FieldError>", "Result<String, Error>", false),
            (
                "Result<i32, FieldError>",
                "Result<String, FieldError>",
                true,
            ),
            ("Result<i32, FieldError>", "Result<String, Error>", true),
            ("Result<Vec<(i32, i32)>, E>", "Result<Vec<String>, E>", true),
            ("Result<Vec<_>, E>", "Result<Vec<String>, E>", false),
            ("Result<_, E>", "Result<String, E>", false),
        ] {
            let record = json!({"message":format!("expected future to be a future that resolves to `{expected}`, but it resolves to `{actual}`")});
            assert_eq!(mismatch(&record), changed, "{record}");
        }
    }

    #[test]
    fn unknown_comparisons_and_unbalanced_types_do_not_guess() {
        assert!(!mismatch(
            &json!({"message":"expected String, found Error"})
        ));
        for result in [
            "String",
            "Result<, E>",
            "Result<T>",
            "Result<A<B), E>",
            "Result<T<{1}>, E>",
        ] {
            assert_eq!(success(result), None, "{result}");
        }
        assert_eq!(
            success("Result<fn() -> Option<i32>, E>"),
            Some("fn() -> Option<i32>")
        );
    }
}
