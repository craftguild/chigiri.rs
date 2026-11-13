//! Keep index-derived records in the same snapshot without copying them, preserving relation wrappers.

use std::sync::Arc;

use super::{ObjectData, OutputObject, OutputValue};
use crate::ir::{FieldId, TypeRef, Wrapper};
use crate::store::{Record, Related, Snapshot};
use crate::value::{ValueBudget, ValueError};

pub(crate) fn project(
    snapshot: &Snapshot,
    field: FieldId,
    related: Related,
    budget: &mut ValueBudget,
) -> Result<OutputValue, ValueError> {
    let definition = snapshot
        .ir()
        .graph()
        .fields()
        .get(field.0)
        .ok_or(ValueError("unknown relation field"))?;
    let ty = &definition.ty;
    budget.container(1)?;
    match related {
        Related::Null if ty.nullable() => Ok(OutputValue::Null),
        Related::One(record) if ty.list_depth() == 0 => attach(snapshot, ty, record),
        Related::Many(records) if ty.list_depth() == 1 => {
            budget.items(records.len())?;
            let position = ty
                .wrappers
                .iter()
                .position(|w| *w == Wrapper::List)
                .unwrap();
            let nullable = ty.wrappers.get(position + 1) != Some(&Wrapper::NonNull);
            let mut result = Vec::with_capacity(records.len());
            for record in records {
                budget.container(2)?;
                result.push(match record {
                    Some(record) => attach(snapshot, ty, record)?,
                    None if nullable => OutputValue::Null,
                    None => return Err(ValueError("non-null relation item required")),
                });
            }
            Ok(OutputValue::List(result))
        }
        _ => Err(ValueError("relation output shape mismatch")),
    }
}

fn attach(
    snapshot: &Snapshot,
    ty: &TypeRef,
    record: Arc<Record>,
) -> Result<OutputValue, ValueError> {
    let ir = snapshot.ir();
    if !record.belongs_to(ir)
        || ir.models()[record.model().0].ty != ty.named
        || !snapshot
            .row(record.model(), record.row())
            .is_some_and(|stored| Arc::ptr_eq(stored, &record))
    {
        return Err(ValueError("relation record differs from snapshot"));
    }
    Ok(OutputValue::Object(OutputObject {
        snapshot: snapshot.clone(),
        ty: ty.named,
        data: ObjectData::Model(record),
    }))
}
