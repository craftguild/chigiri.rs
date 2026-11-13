//! Build read-only observation values for operation and field hooks.

use super::copying::Copy;
use super::session::{FieldView, Location};
use crate::output::OutputObject;
use crate::service::Client;
use crate::value::{ValueBudget, ValueError};
use chigiri::core::Object;
use chigiri::core::executable::CoercedInput;
use chigiri::core::language::ast::OperationType;
use chigiri::{FieldContext, OperationContext, Value};
use std::sync::Arc;

pub(super) fn operation(
    context: &OperationContext<'_, Client>,
    budget: &mut ValueBudget,
) -> Result<Value, ValueError> {
    let mut copy = Copy(budget);
    let kind = copy.text(
        match context.operation_type() {
            OperationType::Query => "query",
            OperationType::Mutation => "mutation",
            OperationType::Subscription => "subscription",
        },
        2,
    )?;
    let name = match context.operation_name() {
        Some(name) => copy.text(name, 2)?,
        None => copy.value(&Value::Null, 2)?,
    };
    copy.0.container(2)?;
    let mut variables = Object::new();
    for (name, value) in context.variables().iter() {
        let value = match value {
            CoercedInput::Undefined => continue,
            CoercedInput::Null => &Value::Null,
            CoercedInput::Value(value) => value,
        };
        copy.0.key(name)?;
        variables.insert(name, copy.value(value, 3)?);
    }
    copy.shape(
        "ChigiriHookOperation",
        vec![kind, name, Value::Object(variables)],
        1,
    )
}

pub(super) fn field(
    context: &FieldContext<'_, Client>,
    budget: &mut ValueBudget,
) -> Result<FieldView, ValueError> {
    let mut copy = Copy(budget);
    copy.0.string_node(
        context.parent_type().len() + context.field_name().len() + 1,
        2,
    )?;
    let coordinate = Value::String(format!(
        "{}.{}",
        context.parent_type(),
        context.field_name()
    ));
    let path = copy.path(context.path(), 2)?;
    let arguments = copy.object(
        context
            .arguments()
            .ok_or(ValueError("field arguments required"))?,
        2,
    )?;
    let parent = match context.parent() {
        Some(parent) => super::observation::object(
            parent
                .downcast_ref::<OutputObject>()
                .ok_or(ValueError("mock parent required"))?,
            &mut copy,
            2,
        )?,
        None => copy.value(&Value::Null, 2)?,
    };
    let coordinate_text = coordinate.as_str().ok_or(ValueError("field coordinate"))?;
    copy.0.string_node(coordinate_text.len(), 1)?;
    let location_coordinate = Some(Arc::from(coordinate_text));
    copy.0.container(1)?;
    copy.0.items(context.path().len())?;
    let mut location_path = Vec::new();
    for part in context.path() {
        match part {
            chigiri::PathSegment::Field(name) => copy.0.string_node(name.len(), 2)?,
            chigiri::PathSegment::Index(_) => copy.0.node(&Value::Int(0), 2)?,
        }
        location_path.push(part.clone());
    }
    let location = Location {
        coordinate: location_coordinate,
        path: location_path.into(),
    };
    let value = copy.shape(
        "ChigiriHookField",
        vec![coordinate, path, arguments, parent],
        1,
    )?;
    Ok(FieldView {
        value: Arc::new(value),
        location,
    })
}
