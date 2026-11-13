//! Encode host query results and nested parents with shared conversion budgets and metadata rules.

use std::sync::Arc;

use chigiri::core::Value;
use luaur::Value as LuaValue;

use super::{Codec, JsonShape, VmValue, hint::Hint};
use crate::ir::{MockIr, ModelId};
use crate::output::{ObjectData, OutputObject, OutputValue};
use crate::store::Record;
use crate::value::ValueError;

#[cfg(test)]
mod tests;

impl Codec {
    pub(crate) fn encode_output(&mut self, output: &OutputValue) -> Result<VmValue, ValueError> {
        self.run(|this| this.output_value(output, 1))
            .map(|value| self.scope.wrap(value))
    }

    /// Decode a relation field name using the schema-name limit rather than the 4 KiB operand limit.
    pub fn decode_name(&mut self, ir: &MockIr, value: &VmValue) -> Result<Value, ValueError> {
        self.decode_hint(ir, Hint::FieldName, value)
    }

    /// Decode query options, predicates, and arrays according to their closed grammar.
    pub fn decode_query(
        &mut self,
        ir: &MockIr,
        model: ModelId,
        count: bool,
        value: &VmValue,
    ) -> Result<Value, ValueError> {
        if ir.models().get(model.0).is_none() {
            self.fail();
            return Err(ValueError("unknown model"));
        }
        self.decode_hint(ir, Hint::Query(count), value)
    }

    /// Encode a record list, charging the list and each record to the value budget.
    pub fn encode_records(
        &mut self,
        ir: &MockIr,
        records: &[Arc<Record>],
    ) -> Result<VmValue, ValueError> {
        self.run(|this| {
            this.budget.container(1)?;
            this.budget.items(records.len())?;
            let table = this.new_table(JsonShape::Array)?;
            for (index, record) in records.iter().enumerate() {
                table
                    .raw_set(index + 1, this.record(ir, record, 2)?)
                    .map_err(|error| this.vm_error(error))?;
            }
            Ok(LuaValue::Table(table))
        })
        .map(|value| self.scope.wrap(value))
    }

    /// Encode only the validated stored record or DTO fields present on the parent.
    pub fn encode_parent(&mut self, parent: &OutputObject) -> Result<VmValue, ValueError> {
        self.run(|this| this.output_object(parent, 1))
            .map(|value| self.scope.wrap(value))
    }

    fn output_object(
        &mut self,
        object: &OutputObject,
        depth: usize,
    ) -> Result<LuaValue, ValueError> {
        let ir = object.snapshot().ir();
        let fields = match object.data() {
            ObjectData::Model(record) => return self.record(ir, record, depth),
            ObjectData::CapturedRecord(record) => {
                return self.captured_record(ir, record, depth);
            }
            ObjectData::Dto(fields) | ObjectData::Stored(fields) => fields,
        };
        self.budget.container(depth)?;
        let table = self.new_table(JsonShape::Object)?;
        let name = &ir.graph().types()[object.ty().0].name;
        self.budget.key("__typename")?;
        self.budget.string_node(name.len(), depth + 1)?;
        table
            .raw_set("__typename", name.as_ref())
            .map_err(|error| self.vm_error(error))?;
        for (id, value) in fields.iter() {
            let name = &ir.graph().fields()[id.0].name;
            self.budget.key(name)?;
            table
                .raw_set(name.as_ref(), self.output_value(value, depth + 1)?)
                .map_err(|error| self.vm_error(error))?;
        }
        Ok(LuaValue::Table(table))
    }

    fn captured_record(
        &mut self,
        ir: &MockIr,
        record: &Arc<crate::output::CapturedRecord>,
        depth: usize,
    ) -> Result<LuaValue, ValueError> {
        if !record.belongs_to(ir) {
            return Err(ValueError("foreign captured record schema"));
        }
        self.budget.container(depth)?;
        let table = self.new_table(JsonShape::Object)?;
        let model = &ir.models()[record.model().0];
        let name = &ir.graph().types()[model.ty.0].name;
        self.budget.key("__typename")?;
        self.budget.string_node(name.len(), depth + 1)?;
        table
            .raw_set("__typename", name.as_ref())
            .map_err(|error| self.vm_error(error))?;
        for (slot, value) in record.values() {
            let name = &ir.slots()[slot.0].name;
            self.budget.key(name)?;
            table
                .raw_set(name.as_ref(), self.encode_value(value, depth + 1)?)
                .map_err(|error| self.vm_error(error))?;
        }
        self.records.insert(
            table.to_pointer() as usize,
            (table.clone(), super::RecordOrigin::Captured(record.clone())),
        );
        Ok(LuaValue::Table(table))
    }

    fn output_value(&mut self, value: &OutputValue, depth: usize) -> Result<LuaValue, ValueError> {
        match value {
            OutputValue::Null => self.encode_value(&Value::Null, depth),
            OutputValue::Scalar(value) => self.encode_value(value, depth),
            OutputValue::Object(object) => self.output_object(object, depth),
            OutputValue::List(items) => {
                self.budget.container(depth)?;
                self.budget.items(items.len())?;
                let table = self.new_table(JsonShape::Array)?;
                for (index, value) in items.iter().enumerate() {
                    table
                        .raw_set(index + 1, self.output_value(value, depth + 1)?)
                        .map_err(|error| self.vm_error(error))?;
                }
                Ok(LuaValue::Table(table))
            }
        }
    }
}
