//! Resolve stored values, DTO fields, and relations from the parent snapshot.

use chigiri::core::{Object, Value};

use crate::ir::{FieldId, FieldPlan};
use crate::output::{ObjectData, OutputObject, OutputValue};
use crate::runtime::input;

#[cfg(test)]
mod tests;
pub(super) enum NativeError {
    Output(crate::output::OutputError),
    Other,
}

pub(super) fn execute(
    field: FieldId,
    arguments: &Object,
    parent: &OutputObject,
    execution: &crate::runtime::Execution,
) -> Result<OutputValue, NativeError> {
    let ir = parent.snapshot().ir();
    let mut reader = parent.snapshot().reader();
    reader.share_work(execution.budget.0.clone());
    reader.execution_guard(&execution.cancel, &execution.shutdown, execution.deadline);
    input::validate(ir, field, arguments, reader.value_budget()).map_err(|_| NativeError::Other)?;
    match (ir.field_plan(field), parent.data()) {
        (Some(FieldPlan::Stored(slot)), ObjectData::Model(record)) => reader
            .stored_output(field, record.get(*slot).unwrap_or(&Value::Null))
            .map_err(NativeError::Output),
        (Some(FieldPlan::Stored(slot)), ObjectData::CapturedRecord(record)) => reader
            .stored_output(field, record.get(*slot).unwrap_or(&Value::Null))
            .map_err(NativeError::Output),
        (
            Some(FieldPlan::Value | FieldPlan::Stored(_)),
            ObjectData::Dto(values) | ObjectData::Stored(values),
        ) => {
            // The DTO was validated as output already, and its object remains bound to this snapshot.
            Ok(values.get(&field).cloned().unwrap_or(OutputValue::Null))
        }
        (Some(FieldPlan::Owner(_) | FieldPlan::Inverse { .. }), ObjectData::Model(record)) => {
            reader
                .related_output(field, record.row())
                .map_err(|_| NativeError::Other)
        }
        (
            Some(FieldPlan::Owner(_) | FieldPlan::Inverse { .. }),
            ObjectData::CapturedRecord(record),
        ) => reader
            .captured_related_output(field, record)
            .map_err(|_| NativeError::Other),
        _ => Err(NativeError::Other),
    }
}
