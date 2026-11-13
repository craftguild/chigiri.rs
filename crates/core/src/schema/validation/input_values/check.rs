//! Iteratively validate value compatibility with expected input types.

use std::collections::BTreeSet;

use crate::language::ast::{Value, ValueNode};
use crate::language::numeric::{valid_float, valid_int};
use crate::schema::{
    SCHEMA_CONST_DUPLICATE_INPUT_FIELD, SCHEMA_CONST_ENUM_VALUE, SCHEMA_CONST_INPUT_OBJECT_VALUE,
    SCHEMA_CONST_MISSING_INPUT_FIELD, SCHEMA_CONST_NULL_FOR_NON_NULL, SCHEMA_CONST_ONE_OF_VALUE,
    SCHEMA_CONST_SCALAR_VALUE, SCHEMA_CONST_UNKNOWN_INPUT_FIELD, SCHEMA_CONST_VALUE_VARIABLE,
};

use super::super::super::composer::Composer;
use super::{Catalog, ExpectedType, InputObjectShape, Job, NamedShape, ScalarShape};

struct Work<'value, 'type_> {
    value: &'value ValueNode,
    expected: &'type_ ExpectedType,
}

pub(super) fn validate(composer: &mut Composer, catalog: &Catalog, job: &Job) {
    let mut work = vec![Work {
        value: &job.value,
        expected: &job.expected,
    }];
    while let Some(current) = work.pop() {
        check_value(composer, catalog, current, &mut work);
        if !composer.can_continue() {
            return;
        }
    }
}

fn check_value<'value, 'type_>(
    composer: &mut Composer,
    catalog: &'type_ Catalog,
    current: Work<'value, 'type_>,
    work: &mut Vec<Work<'value, 'type_>>,
) {
    if let Value::Variable(name) = &current.value.value {
        composer.report(
            SCHEMA_CONST_VALUE_VARIABLE,
            format!(
                "Schema constant value cannot contain variable `${}`",
                name.value
            ),
            current.value.span,
        );
        return;
    }
    match current.expected {
        ExpectedType::NonNull(inner) => {
            if matches!(current.value.value, Value::Null) {
                composer.report(
                    SCHEMA_CONST_NULL_FOR_NON_NULL,
                    "Non-null input type cannot accept null",
                    current.value.span,
                );
            } else {
                work.push(Work {
                    value: current.value,
                    expected: inner,
                });
            }
        }
        ExpectedType::List(item) => {
            if matches!(current.value.value, Value::Null) {
                return;
            }
            if let Value::List(items) = &current.value.value {
                for value in items.iter().rev() {
                    work.push(Work {
                        value,
                        expected: item,
                    });
                }
            } else {
                work.push(Work {
                    value: current.value,
                    expected: item,
                });
            }
        }
        ExpectedType::Named(name) => {
            if matches!(current.value.value, Value::Null) {
                return;
            }
            match catalog.get(name) {
                Some(NamedShape::Scalar(shape)) => check_scalar(composer, shape, current.value),
                Some(NamedShape::Enum(values)) => check_enum(composer, name, values, current.value),
                Some(NamedShape::InputObject(shape)) => {
                    check_input_object(composer, name, shape, current.value, work)
                }
                None => {}
            }
        }
    }
}

fn check_scalar(composer: &mut Composer, shape: &ScalarShape, value: &ValueNode) {
    if matches!(shape, ScalarShape::Custom) {
        reject_nested_variables(composer, value);
        return;
    }
    let valid = match (shape, &value.value) {
        (ScalarShape::Int, Value::Int(raw)) => valid_int(raw) && raw.parse::<i32>().is_ok(),
        (ScalarShape::Float, Value::Int(raw)) => {
            valid_int(raw) && crate::numeric::exact_float_from_integer_lexeme(raw).is_some()
        }
        (ScalarShape::Float, Value::Float(raw)) => valid_float(raw) && finite_number(raw),
        (ScalarShape::String, Value::String(_)) => true,
        (ScalarShape::Boolean, Value::Boolean(_)) => true,
        (ScalarShape::Id, Value::String(_)) => true,
        (ScalarShape::Id, Value::Int(raw)) => valid_int(raw),
        _ => false,
    };
    if !valid {
        composer.report(
            SCHEMA_CONST_SCALAR_VALUE,
            "Constant value is incompatible with the expected scalar type",
            value.span,
        );
    }
}

fn check_enum(
    composer: &mut Composer,
    type_name: &str,
    values: &BTreeSet<String>,
    value: &ValueNode,
) {
    let valid = matches!(&value.value, Value::Enum(name) if values.contains(&name.value));
    if !valid {
        composer.report(
            SCHEMA_CONST_ENUM_VALUE,
            format!("Constant value is not a defined member of enum `{type_name}`"),
            value.span,
        );
    }
}

fn check_input_object<'value, 'type_>(
    composer: &mut Composer,
    type_name: &str,
    shape: &'type_ InputObjectShape,
    value: &'value ValueNode,
    work: &mut Vec<Work<'value, 'type_>>,
) {
    let Value::Object(entries) = &value.value else {
        composer.report(
            SCHEMA_CONST_INPUT_OBJECT_VALUE,
            format!("Expected an input object value for `{type_name}`"),
            value.span,
        );
        return;
    };
    if shape.one_of && (entries.len() != 1 || matches!(entries[0].value.value, Value::Null)) {
        composer.report(
            SCHEMA_CONST_ONE_OF_VALUE,
            format!("OneOf input object `{type_name}` requires exactly one non-null field"),
            value.span,
        );
        if !composer.can_continue() {
            return;
        }
    }
    let mut seen = BTreeSet::new();
    let mut nested = Vec::new();
    for entry in entries {
        if !seen.insert(entry.name.value.as_str()) {
            composer.report(
                SCHEMA_CONST_DUPLICATE_INPUT_FIELD,
                format!(
                    "Input field `{}` is provided more than once",
                    entry.name.value
                ),
                entry.name.span,
            );
        } else if let Some(field) = shape.fields.get(&entry.name.value) {
            nested.push((&entry.value, &field.ty));
        } else {
            composer.report(
                SCHEMA_CONST_UNKNOWN_INPUT_FIELD,
                format!(
                    "Input object `{type_name}` has no field `{}`",
                    entry.name.value
                ),
                entry.name.span,
            );
        }
        if !composer.can_continue() {
            return;
        }
    }
    for required in &shape.required {
        if !seen.contains(required.as_str()) {
            composer.report(
                SCHEMA_CONST_MISSING_INPUT_FIELD,
                format!("Input object `{type_name}` requires field `{required}`"),
                value.span,
            );
            if !composer.can_continue() {
                return;
            }
        }
    }
    for (nested_value, expected) in nested.into_iter().rev() {
        work.push(Work {
            value: nested_value,
            expected,
        });
    }
}

fn reject_nested_variables(composer: &mut Composer, value: &ValueNode) {
    let mut work = vec![value];
    while let Some(current) = work.pop() {
        match &current.value {
            Value::Variable(name) => composer.report(
                SCHEMA_CONST_VALUE_VARIABLE,
                format!(
                    "Schema constant value cannot contain variable `${}`",
                    name.value
                ),
                current.span,
            ),
            Value::List(items) => work.extend(items.iter().rev()),
            Value::Object(fields) => {
                work.extend(fields.iter().rev().map(|field| &field.value));
            }
            Value::Int(_)
            | Value::Float(_)
            | Value::String(_)
            | Value::Boolean(_)
            | Value::Null
            | Value::Enum(_) => {}
        }
        if !composer.can_continue() {
            return;
        }
    }
}

fn finite_number(raw: &str) -> bool {
    crate::numeric::finite_float_from_lexeme(raw).is_some()
}
