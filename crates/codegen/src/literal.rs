//! Convert GraphQL values embedded in descriptors to canonical text.

use std::fmt::Write as _;

use chigiri_core::language::ast::{Value, ValueNode};

pub(crate) fn graphql_value(value: &ValueNode) -> Result<String, String> {
    render_value(&value.value)
}

pub(crate) fn semantic_value(value: &Value) -> Result<String, String> {
    render_value(value)
}

fn render_value(value: &Value) -> Result<String, String> {
    match value {
        Value::Variable(name) => Err(format!(
            "SF4007: Const value is mixed with variable `${}`.",
            name.value
        )),
        Value::Int(value) | Value::Float(value) => Ok(value.clone()),
        Value::String(value) => Ok(graphql_string(&value.value)),
        Value::Boolean(value) => Ok(value.to_string()),
        Value::Null => Ok("null".to_owned()),
        Value::Enum(name) => Ok(name.value.clone()),
        Value::List(items) => {
            let items = items
                .iter()
                .map(graphql_value)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("[{}]", items.join(", ")))
        }
        Value::Object(fields) => {
            let fields = fields
                .iter()
                .map(|field| {
                    graphql_value(&field.value)
                        .map(|value| format!("{}: {value}", field.name.value))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("{{{}}}", fields.join(", ")))
        }
    }
}

fn graphql_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len().saturating_add(2));
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{8}' => output.push_str("\\b"),
            '\u{c}' => output.push_str("\\f"),
            character if character.is_control() => {
                write!(&mut output, "\\u{:04X}", u32::from(character))
                    .expect("writing to a String cannot fail");
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::graphql_string;

    #[test]
    fn canonically_escapes_graphql_strings() {
        assert_eq!(
            graphql_string("a\"\\\n\t\u{7}"),
            "\"a\\\"\\\\\\n\\t\\u0007\""
        );
    }
}
