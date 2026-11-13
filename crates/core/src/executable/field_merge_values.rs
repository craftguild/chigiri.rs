//! Span-independent argument and input-value normalization for field-merge comparison.

use crate::language::ast::{Field, Value};

pub(super) fn field_signature(field: &Field) -> String {
    let mut arguments = field
        .arguments
        .iter()
        .map(|argument| {
            (
                argument.name.value.as_str(),
                value_key(&argument.value.value),
            )
        })
        .collect::<Vec<_>>();
    arguments.sort();
    let mut key = atom("f", &field.name.value);
    for (name, value) in arguments {
        key.push_str(&atom("a", name));
        key.push_str(&atom("=", &value));
    }
    key
}

enum Work<'a> {
    Visit(&'a Value),
    FinishList(usize),
    FinishObject(Vec<&'a str>),
}

fn value_key(value: &Value) -> String {
    let mut results = Vec::new();
    let mut work = vec![Work::Visit(value)];
    while let Some(item) = work.pop() {
        match item {
            Work::Visit(value) => visit(value, &mut work, &mut results),
            Work::FinishList(length) => {
                let values = results.split_off(results.len() - length);
                let mut key = String::from("l");
                for value in values {
                    key.push_str(&atom("v", &value));
                }
                results.push(key);
            }
            Work::FinishObject(names) => {
                let values = results.split_off(results.len() - names.len());
                let mut key = String::from("o");
                for (name, value) in names.into_iter().zip(values) {
                    key.push_str(&atom("k", name));
                    key.push_str(&atom("v", &value));
                }
                results.push(key);
            }
        }
    }
    results.pop().unwrap_or_default()
}

fn visit<'a>(value: &'a Value, work: &mut Vec<Work<'a>>, results: &mut Vec<String>) {
    match value {
        Value::Variable(name) => results.push(atom("v", &name.value)),
        Value::Int(raw) => results.push(atom("i", raw)),
        Value::Float(raw) => results.push(atom("f", raw)),
        Value::String(value) => results.push(atom("s", &value.value)),
        Value::Boolean(value) => results.push(if *value { "b1" } else { "b0" }.to_owned()),
        Value::Null => results.push("n".to_owned()),
        Value::Enum(name) => results.push(atom("e", &name.value)),
        Value::List(values) => {
            work.push(Work::FinishList(values.len()));
            for value in values.iter().rev() {
                work.push(Work::Visit(&value.value));
            }
        }
        Value::Object(fields) => {
            let mut fields = fields.iter().collect::<Vec<_>>();
            fields.sort_by(|left, right| left.name.value.cmp(&right.name.value));
            work.push(Work::FinishObject(
                fields
                    .iter()
                    .map(|field| field.name.value.as_str())
                    .collect(),
            ));
            for field in fields.into_iter().rev() {
                work.push(Work::Visit(&field.value.value));
            }
        }
    }
}

fn atom(kind: &str, value: &str) -> String {
    format!("{kind}{}:{value}", value.len())
}
