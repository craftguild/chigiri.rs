//! Charge Rust value nodes, keys, and bytes before copying them into the VM.

use chigiri::core::{Object, Value};
use luaur::{Table, Value as LuaValue};
use std::sync::Arc;

use super::{Codec, JsonShape};
use crate::ir::MockIr;
use crate::store::Record;
use crate::value::{MAX_STRING_BYTES, ValueError, exact_number};

impl Codec {
    pub(super) fn encode_value(
        &mut self,
        value: &Value,
        depth: usize,
    ) -> Result<LuaValue, ValueError> {
        self.encode_limited(value, depth, MAX_STRING_BYTES)
    }

    pub(super) fn encode_limited(
        &mut self,
        value: &Value,
        depth: usize,
        string_limit: usize,
    ) -> Result<LuaValue, ValueError> {
        self.budget.node(value, depth)?;
        match value {
            Value::Null => Ok(self.null_lua()),
            Value::Boolean(value) => Ok(LuaValue::Boolean(*value)),
            Value::Int(_) | Value::Float(_) | Value::BigInt(_) => {
                Ok(LuaValue::Number(exact_number(value)?))
            }
            Value::String(value) => {
                if value.len() > string_limit {
                    return Err(ValueError("string length"));
                }
                Ok(LuaValue::String(self.lua.create_string(value)))
            }
            Value::List(items) => {
                self.budget.items(items.len())?;
                let table = self.new_table(JsonShape::Array)?;
                for (index, value) in items.iter().enumerate() {
                    let value = self.encode_limited(value, depth + 1, string_limit)?;
                    table
                        .raw_set(index + 1, value)
                        .map_err(|error| self.vm_error(error))?;
                }
                Ok(LuaValue::Table(table))
            }
            Value::Object(object) => self.object_limited(object, depth, string_limit),
        }
    }

    pub(super) fn object_value(
        &mut self,
        object: &Object,
        depth: usize,
    ) -> Result<LuaValue, ValueError> {
        self.object_limited(object, depth, MAX_STRING_BYTES)
    }

    fn object_limited(
        &mut self,
        object: &Object,
        depth: usize,
        string_limit: usize,
    ) -> Result<LuaValue, ValueError> {
        let table = self.new_table(JsonShape::Object)?;
        for (name, value) in object {
            self.budget.key(name)?;
            let value = self.encode_limited(value, depth + 1, string_limit)?;
            table
                .raw_set(name, value)
                .map_err(|error| self.vm_error(error))?;
        }
        Ok(LuaValue::Table(table))
    }

    pub(super) fn record(
        &mut self,
        ir: &MockIr,
        record: &Arc<Record>,
        depth: usize,
    ) -> Result<LuaValue, ValueError> {
        if !record.belongs_to(ir) {
            return Err(ValueError("foreign schema record"));
        }
        let model = ir
            .models()
            .get(record.model().0)
            .ok_or(ValueError("unknown model"))?;
        self.budget.container(depth)?;
        let table = self.new_table(JsonShape::Object)?;
        let name = &ir.graph().types()[model.ty.0].name;
        self.budget.key(crate::api::model_values::TYPENAME)?;
        self.budget.string_node(name.len(), depth + 1)?;
        table
            .raw_set(crate::api::model_values::TYPENAME, name.as_ref())
            .map_err(|error| self.vm_error(error))?;
        for (id, value) in record.values() {
            let name = &ir.slots()[id.0].name;
            self.budget.key(name)?;
            let value = self.encode_value(value, depth + 1)?;
            table
                .raw_set(name.as_ref(), value)
                .map_err(|error| self.vm_error(error))?;
        }
        self.records.insert(
            table.to_pointer() as usize,
            (table.clone(), super::RecordOrigin::Live(record.clone())),
        );
        Ok(LuaValue::Table(table))
    }

    pub(super) fn new_table(&self, shape: JsonShape) -> Result<Table, ValueError> {
        let table = self
            .lua
            .create_table_result()
            .map_err(|error| self.vm_error(error))?;
        table
            .set_metatable(Some(match shape {
                JsonShape::Object => self.object.clone(),
                JsonShape::Array => self.array.clone(),
            }))
            .map_err(|error| self.vm_error(error))?;
        Ok(table)
    }
}
