//! Validates opaque custom-scalar literals.

use std::collections::BTreeSet;

use crate::language::ast::{Name, Value as AstValue, ValueNode};
use crate::{Object, Span, Value};

pub(super) struct Scan<'a> {
    pub(super) variables: Vec<&'a Name>,
    pub(super) has_duplicates: bool,
}

/// Walks custom-scalar values, reporting variables and duplicate fields until stopped.
pub(super) fn scan<F>(root: &ValueNode, mut report_duplicate: F) -> Scan<'_>
where
    F: FnMut(&str, Span) -> bool,
{
    let mut variables = Vec::new();
    let mut has_duplicates = false;
    let mut work = vec![root];
    while let Some(value) = work.pop() {
        match &value.value {
            AstValue::Variable(name) => variables.push(name),
            AstValue::List(items) => work.extend(items.iter().rev()),
            AstValue::Object(fields) => {
                let mut seen = BTreeSet::new();
                for field in fields {
                    #[cfg(test)]
                    super::metrics::record_custom_field();
                    if !seen.insert(field.name.value.as_str()) {
                        has_duplicates = true;
                        if !report_duplicate(&field.name.value, field.name.span) {
                            return Scan {
                                variables,
                                has_duplicates,
                            };
                        }
                    }
                }
                work.extend(fields.iter().rev().map(|field| &field.value));
            }
            AstValue::Int(_)
            | AstValue::Float(_)
            | AstValue::String(_)
            | AstValue::Boolean(_)
            | AstValue::Null
            | AstValue::Enum(_) => {}
        }
    }
    Scan {
        variables,
        has_duplicates,
    }
}

enum Task<'a> {
    Visit(&'a ValueNode),
    FinishList(usize),
    FinishObject(Vec<String>, usize),
}

/// Convert the constant literal to the core value, which does not contain a variable and overlap.
pub(super) fn normalize(root: &ValueNode) -> Result<Value, Span> {
    let mut work = vec![Task::Visit(root)];
    let mut values = Vec::new();
    while let Some(task) = work.pop() {
        match task {
            Task::Visit(node) => match &node.value {
                AstValue::Variable(_) => return Err(node.span),
                AstValue::Int(raw) => {
                    values.push(crate::numeric::value_from_integer_lexeme(raw).ok_or(node.span)?);
                }
                AstValue::Float(raw) => {
                    let value = crate::numeric::finite_float_from_lexeme(raw).ok_or(node.span)?;
                    values.push(Value::Float(value));
                }
                AstValue::String(value) => values.push(Value::String(value.value.clone())),
                AstValue::Boolean(value) => values.push(Value::Boolean(*value)),
                AstValue::Null => values.push(Value::Null),
                AstValue::Enum(name) => values.push(Value::String(name.value.clone())),
                AstValue::List(items) => {
                    work.push(Task::FinishList(items.len()));
                    work.extend(items.iter().rev().map(Task::Visit));
                }
                AstValue::Object(fields) => {
                    work.push(Task::FinishObject(
                        fields
                            .iter()
                            .map(|field| field.name.value.clone())
                            .collect(),
                        fields.len(),
                    ));
                    work.extend(fields.iter().rev().map(|field| Task::Visit(&field.value)));
                }
            },
            Task::FinishList(length) => {
                let start = values.len().saturating_sub(length);
                let items = values.split_off(start);
                values.push(Value::List(items));
            }
            Task::FinishObject(names, length) => {
                let start = values.len().saturating_sub(length);
                let members = values.split_off(start);
                let mut object = Object::with_capacity(length);
                object.extend(names.into_iter().zip(members));
                values.push(Value::Object(object));
            }
        }
    }
    values.pop().ok_or(root.span)
}
