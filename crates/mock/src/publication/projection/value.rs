//! Charge a fresh event budget before copying canonical scalar and list values.

use chigiri::core::{Object, Value};

use super::object;
use crate::ir::{TypeId, TypeRef, Wrapper};
use crate::output::OutputValue;
use crate::publication::capture::{CapturedField, CapturedValue};
use crate::store::Snapshot;
use crate::value::{ValueBudget, ValueError};

pub(super) fn typed(
    captured: &CapturedValue,
    snapshot: &Snapshot,
    ty: &TypeRef,
    budget: &mut ValueBudget,
    depth: usize,
) -> Result<OutputValue, ValueError> {
    typed_parts(captured, snapshot, ty.named, &ty.wrappers, budget, depth)
}

fn typed_parts(
    captured: &CapturedValue,
    snapshot: &Snapshot,
    named: TypeId,
    mut wrappers: &[Wrapper],
    budget: &mut ValueBudget,
    depth: usize,
) -> Result<OutputValue, ValueError> {
    if wrappers.first() == Some(&Wrapper::NonNull) {
        if matches!(captured, CapturedValue::Null) {
            return Err(ValueError("null captured for non-null output"));
        }
        wrappers = &wrappers[1..];
    } else if matches!(captured, CapturedValue::Null) {
        budget.node(&Value::Null, depth)?;
        return Ok(OutputValue::Null);
    }
    if wrappers.first() == Some(&Wrapper::List) {
        let CapturedValue::List(items) = captured else {
            return Err(ValueError("captured output list required"));
        };
        budget.container(depth)?;
        budget.items(items.len())?;
        let mut projected = reserved(items.len())?;
        for item in items {
            projected.push(typed_parts(
                item,
                snapshot,
                named,
                &wrappers[1..],
                budget,
                depth + 1,
            )?);
        }
        return Ok(OutputValue::List(projected));
    }
    let node = snapshot
        .ir()
        .graph()
        .types()
        .get(named.0)
        .ok_or(ValueError("unknown captured output type"))?;
    match node.kind {
        chigiri::core::schema::TypeKind::Scalar | chigiri::core::schema::TypeKind::Enum => {
            scalar(captured, budget, depth).map(OutputValue::Scalar)
        }
        chigiri::core::schema::TypeKind::Object
        | chigiri::core::schema::TypeKind::Interface
        | chigiri::core::schema::TypeKind::Union => {
            let CapturedValue::Object(value) = captured else {
                return Err(ValueError("captured output object required"));
            };
            object::project(value, snapshot, named, budget, depth)
        }
        _ => Err(ValueError("captured output type required")),
    }
}

pub(super) fn scalar(
    captured: &CapturedValue,
    budget: &mut ValueBudget,
    depth: usize,
) -> Result<Value, ValueError> {
    match captured {
        CapturedValue::Null => {
            budget.node(&Value::Null, depth)?;
            Ok(Value::Null)
        }
        CapturedValue::Boolean(value) => {
            budget.node(&Value::Boolean(*value), depth)?;
            Ok(Value::Boolean(*value))
        }
        CapturedValue::Integer(value) => {
            let value = Value::Int((*value).into());
            budget.node(&value, depth)?;
            Ok(value)
        }
        CapturedValue::Float(bits) => {
            let value = f64::from_bits(*bits);
            if !value.is_finite() {
                return Err(ValueError("invalid captured float"));
            }
            let value = Value::Float(value);
            budget.node(&value, depth)?;
            Ok(value)
        }
        CapturedValue::BigInt(value) => {
            budget.string_node(value.len(), depth)?;
            Ok(Value::BigInt(owned(value)?))
        }
        CapturedValue::String(value) => {
            budget.string_node(value.len(), depth)?;
            Ok(Value::String(owned(value)?))
        }
        CapturedValue::List(items) => {
            budget.container(depth)?;
            budget.items(items.len())?;
            let mut values = reserved(items.len())?;
            for item in items {
                values.push(scalar(item, budget, depth + 1)?);
            }
            Ok(Value::List(values))
        }
        CapturedValue::Map(fields) => map(fields, budget, depth),
        CapturedValue::Object(_) => Err(ValueError("captured object is not a scalar")),
    }
}

fn map(
    fields: &[CapturedField],
    budget: &mut ValueBudget,
    depth: usize,
) -> Result<Value, ValueError> {
    budget.container(depth)?;
    budget.items(fields.len())?;
    let mut result = Object::new();
    for field in fields {
        budget.key_bytes(field.name.len())?;
        let name = owned(&field.name)?;
        if result
            .insert(name, scalar(&field.value, budget, depth + 1)?)
            .is_some()
        {
            return Err(ValueError("duplicate captured map field"));
        }
    }
    Ok(Value::Object(result))
}

fn owned(value: &str) -> Result<String, ValueError> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(value.len())
        .map_err(|_| ValueError("captured value allocation"))?;
    owned.push_str(value);
    Ok(owned)
}

fn reserved<T>(len: usize) -> Result<Vec<T>, ValueError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| ValueError("captured value allocation"))?;
    Ok(values)
}
