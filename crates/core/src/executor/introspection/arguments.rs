//! Coerce field arguments from internal introspection descriptors.

use std::collections::BTreeMap;

use crate::executable::CoercedInput;
use crate::language::ast::{Argument, Field, Value as AstValue};
use crate::schema::introspection::model::{
    DefaultValueDescriptor, FieldDescriptor, InputValueDescriptor, TypeWrapper,
};
use crate::{GraphQlError, Object, PathSegment, Value};

use crate::executor::engine::Engine;

pub(super) fn coerce<C>(
    engine: &Engine<'_, C>,
    definition: &FieldDescriptor,
    field: &Field,
    path: &[PathSegment],
) -> Result<Object, Vec<GraphQlError>> {
    let provided = field
        .arguments
        .iter()
        .map(|argument| (argument.name.value.as_str(), argument))
        .collect::<BTreeMap<_, _>>();
    let mut output = Object::with_capacity(definition.arguments.len());
    let mut errors = Vec::new();
    for argument in definition.arguments {
        match value(engine, argument, provided.get(argument.name).copied()) {
            Ok(CoercedInput::Value(value)) => {
                output.insert(argument.name, value);
            }
            Ok(CoercedInput::Null) => {
                if is_non_null(argument) {
                    errors.push(error(engine, field, path, argument, "cannot be null"));
                } else {
                    output.insert(argument.name, Value::Null);
                }
            }
            Ok(CoercedInput::Undefined) => {
                if is_non_null(argument) {
                    errors.push(error(engine, field, path, argument, "was not provided"));
                }
            }
            Err(message) => errors.push(error(engine, field, path, argument, message)),
        }
        if engine.input_budget.is_some_and(|budget| budget.exhausted())
            || engine.output_bytes_exhausted()
        {
            return Err(errors);
        }
    }
    if errors.is_empty() {
        Ok(output)
    } else {
        Err(errors)
    }
}

fn value<C>(
    engine: &Engine<'_, C>,
    definition: &InputValueDescriptor,
    provided: Option<&Argument>,
) -> Result<CoercedInput, &'static str> {
    let limit = "exceeds the input coercion resource limit";
    if engine
        .input_budget
        .is_some_and(|budget| !budget.claim(1, definition.name.len()))
    {
        return Err(limit);
    }
    let Some(provided) = provided else {
        return default_value(engine, definition);
    };
    let input = match &provided.value.value {
        AstValue::Variable(name) => match engine.variables.get(&name.value) {
            Some(CoercedInput::Value(value)) => {
                if engine
                    .input_budget
                    .is_some_and(|budget| !budget.claim_value(value))
                {
                    return Err(limit);
                }
                CoercedInput::Value(value.clone())
            }
            Some(value) => value.clone(),
            None => CoercedInput::Undefined,
        },
        AstValue::Null => CoercedInput::Null,
        AstValue::Boolean(value) => {
            reserve(engine, 1)?;
            CoercedInput::Value(Value::Boolean(*value))
        }
        AstValue::String(value) => {
            reserve(engine, value.value.len())?;
            CoercedInput::Value(Value::String(value.value.clone()))
        }
        _ => return Err("has an incompatible scalar value"),
    };
    match input {
        CoercedInput::Undefined => default_value(engine, definition),
        CoercedInput::Value(value) if scalar_matches(definition.ty.named_type, &value) => {
            Ok(CoercedInput::Value(value))
        }
        CoercedInput::Value(_) => Err("has an incompatible scalar value"),
        CoercedInput::Null => Ok(CoercedInput::Null),
    }
}

fn reserve<C>(engine: &Engine<'_, C>, bytes: usize) -> Result<(), &'static str> {
    if engine
        .input_budget
        .is_some_and(|budget| !budget.claim(1, bytes))
    {
        Err("exceeds the input coercion resource limit")
    } else {
        Ok(())
    }
}

fn default_value<C>(
    engine: &Engine<'_, C>,
    definition: &InputValueDescriptor,
) -> Result<CoercedInput, &'static str> {
    Ok(match definition.default_value {
        Some(DefaultValueDescriptor::Boolean(value)) => {
            reserve(engine, 1)?;
            CoercedInput::Value(Value::Boolean(value))
        }
        None => CoercedInput::Undefined,
    })
}

fn scalar_matches(name: &str, value: &Value) -> bool {
    matches!(
        (name, value),
        ("String", Value::String(_)) | ("Boolean", Value::Boolean(_))
    )
}

fn is_non_null(definition: &InputValueDescriptor) -> bool {
    definition.ty.wrappers.first() == Some(&TypeWrapper::NonNull)
}

fn error<C>(
    engine: &Engine<'_, C>,
    field: &Field,
    path: &[PathSegment],
    argument: &InputValueDescriptor,
    detail: &str,
) -> GraphQlError {
    if !engine.claim_output_path(path)
        || !engine
            .claim_output_bytes(argument.name.len() + field.name.value.len() + detail.len() + 40)
    {
        return GraphQlError::new("Input error metadata resource limit").code("BAD_USER_INPUT");
    }
    let mut error = GraphQlError::new(format!(
        "Argument `{}` on introspection field `{}` {detail}",
        argument.name, field.name.value
    ))
    .code("BAD_USER_INPUT");
    error.path = path.to_vec();
    if let Some(location) = engine.source.location(field.name.span.start) {
        error.locations.push(location);
    }
    error
}
