//! Copy stored values and DTO resolver outcomes for observation before child selection runs.

use chigiri::core::Object;
use chigiri::core::executor::ResolvedValue;
use chigiri::{FieldResult, GraphQlError, GraphQlResponse, Value};

use super::copying::Copy;
use crate::output::{ObjectData, OutputObject, OutputValue};
use crate::value::{ValueBudget, ValueError};

#[cfg(test)]
mod tests;

pub(super) fn response(
    response: &GraphQlResponse,
    budget: &mut ValueBudget,
) -> Result<Value, ValueError> {
    let mut copy = Copy(budget);
    let data = copy.value(response.data.as_ref().unwrap_or(&Value::Null), 2)?;
    let errors = errors(&response.errors, &mut copy, 2)?;
    copy.shape("ChigiriHookResponse", vec![data, errors], 1)
}

pub(super) fn outcome(
    result: &FieldResult<ResolvedValue>,
    path: &[chigiri::PathSegment],
    budget: &mut ValueBudget,
) -> Result<Value, ValueError> {
    let mut copy = Copy(budget);
    let (value, errors) = match result {
        Ok(value) => (resolved(value, &mut copy, 2)?, errors(&[], &mut copy, 2)?),
        Err(error) => {
            let (message, code) = error.safe_summary();
            let message = copy.text(message, 4)?;
            let code = copy.text(code.unwrap_or("GRAPHQL_ERROR"), 4)?;
            let path = copy.path(path, 4)?;
            let error = copy.shape("ChigiriHookError", vec![message, code, path], 3)?;
            copy.0.container(2)?;
            (copy.value(&Value::Null, 2)?, Value::List(vec![error]))
        }
    };
    copy.shape("ChigiriHookFieldOutcome", vec![value, errors], 1)
}

fn errors(errors: &[GraphQlError], copy: &mut Copy<'_>, depth: usize) -> Result<Value, ValueError> {
    copy.0.container(depth)?;
    copy.0.items(errors.len())?;
    let mut values = Vec::new();
    for error in errors {
        let message = copy.text(&error.message, depth + 2)?;
        let code = copy.text(
            error
                .extensions
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("GRAPHQL_ERROR"),
            depth + 2,
        )?;
        let path = copy.path(&error.path, depth + 2)?;
        values.push(copy.shape("ChigiriHookError", vec![message, code, path], depth + 1)?);
    }
    Ok(Value::List(values))
}

fn resolved(value: &ResolvedValue, copy: &mut Copy<'_>, depth: usize) -> Result<Value, ValueError> {
    match value {
        ResolvedValue::Null => copy.value(&Value::Null, depth),
        ResolvedValue::Leaf(value) => copy.value(value, depth),
        ResolvedValue::Object(value) => object(
            value
                .downcast_ref::<OutputObject>()
                .ok_or(ValueError("mock outcome required"))?,
            copy,
            depth,
        ),
        ResolvedValue::List(values) => {
            copy.0.container(depth)?;
            copy.0.items(values.len())?;
            values
                .iter()
                .map(|value| match value {
                    Ok(value) => resolved(value, copy, depth + 1),
                    Err(_) => Err(ValueError("list outcome conversion failed")),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List)
        }
    }
}

pub(super) fn object(
    value: &OutputObject,
    copy: &mut Copy<'_>,
    depth: usize,
) -> Result<Value, ValueError> {
    copy.0.container(depth)?;
    let ir = value.snapshot().ir();
    let mut object = Object::new();
    copy.0.key("__typename")?;
    object.insert(
        "__typename",
        copy.text(&ir.graph().types()[value.ty().0].name, depth + 1)?,
    );
    match value.data() {
        ObjectData::Model(record) => {
            for (slot, value) in record.values() {
                let name = &ir.slots()[slot.0].name;
                copy.0.key(name)?;
                object.insert(name.to_string(), copy.value(value, depth + 1)?);
            }
        }
        ObjectData::CapturedRecord(record) => {
            for (slot, value) in record.values() {
                let name = &ir.slots()[slot.0].name;
                copy.0.key(name)?;
                object.insert(name.to_string(), copy.value(value, depth + 1)?);
            }
        }
        ObjectData::Dto(fields) | ObjectData::Stored(fields) => {
            for (field, value) in fields.iter() {
                let name = &ir.graph().fields()[field.0].name;
                copy.0.key(name)?;
                object.insert(name.to_string(), output(value, copy, depth + 1)?);
            }
        }
    }
    Ok(Value::Object(object))
}

fn output(value: &OutputValue, copy: &mut Copy<'_>, depth: usize) -> Result<Value, ValueError> {
    match value {
        OutputValue::Null => copy.value(&Value::Null, depth),
        OutputValue::Scalar(value) => copy.value(value, depth),
        OutputValue::Object(value) => object(value, copy, depth),
        OutputValue::List(values) => {
            copy.0.container(depth)?;
            copy.0.items(values.len())?;
            values
                .iter()
                .map(|value| output(value, copy, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List)
        }
    }
}
