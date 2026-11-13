//! Identify null and JSON shapes inside the isolated VM, then bound copies into Rust with one shared budget.
//! Raw Lua handles are internal to runtime workers and never accepted as external input.

mod decode;
mod encode;
mod event;
pub(crate) use event::EventCandidate;
mod hint;
mod hook;
mod host;
mod host_array;
mod provenance;
pub(crate) mod scan;
mod vm_value;
pub(crate) use provenance::{PathPart, Provenance, RecordOrigin};
pub use vm_value::{VmScope, VmValue};
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_limits;
#[cfg(test)]
mod tests_memory;
#[cfg(test)]
mod tests_support;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use chigiri::core::Value;
use luaur::{AnyUserData, Function, Lua, Table, UserData, Value as LuaValue};

use crate::ir::{MockIr, ModelId, TypeRef};
use crate::store::Record;
use crate::value::{ValueBudget, ValueError};

use hint::Hint;

#[derive(Clone, Copy)]
pub(crate) enum HostStringKind {
    Key,
    Value,
}

struct Null;
impl UserData for Null {}

/// JSON container shape applied by the host.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum JsonShape {
    /// JSON object.
    Object,
    /// JSON array.
    Array,
}

/// Codec that owns all VM handles so only validated Rust values cross a `Send` boundary.
pub struct Codec {
    lua: Lua,
    scope: VmScope,
    null: AnyUserData,
    object: Table,
    array: Table,
    string_len: Function,
    budget: ValueBudget,
    records: BTreeMap<usize, (Table, RecordOrigin)>,
    failed: bool,
}

impl Codec {
    pub(crate) fn share_work(&mut self, work: crate::value::work::Work) {
        self.budget.share_work(work);
    }

    /// Capture the standard C string-length function before the sandbox seals globals.
    #[cfg(test)]
    pub fn new(lua: &Lua) -> Result<Self, ValueError> {
        Self::new_with_work(lua, crate::value::work::Work::default())
    }

    /// Create a codec with request work shared from the start, before VM values are converted.
    pub(crate) fn new_with_work(
        lua: &Lua,
        work: crate::value::work::Work,
    ) -> Result<Self, ValueError> {
        catch_unwind(AssertUnwindSafe(|| Self::create(lua, work)))
            .unwrap_or(Err(ValueError("VM allocation failed")))
    }

    fn create(lua: &Lua, work: crate::value::work::Work) -> Result<Self, ValueError> {
        let vm_error = |error| observed_vm_error(error, &work);
        if lua.used_memory() > crate::value::MAX_BYTES {
            work.clone()
                .fail_local(crate::limits::request::LocalLimit::VmMemory);
            return Err(ValueError("VM memory limit"));
        }
        lua.set_memory_limit(crate::value::MAX_BYTES)
            .map_err(vm_error)?;
        let string: Table = lua.globals().get("string").map_err(vm_error)?;
        let marker = || -> Result<Table, ValueError> {
            let table = lua.create_table_result().map_err(vm_error)?;
            table.raw_set("__metatable", false).map_err(vm_error)?;
            table.set_readonly(true);
            Ok(table)
        };
        let mut budget = ValueBudget::default();
        budget.share_work(work.clone());
        Ok(Self {
            lua: lua.clone(),
            scope: VmScope::new(lua),
            null: lua.create_userdata(Null).map_err(vm_error)?,
            object: marker()?,
            array: marker()?,
            string_len: string.get("len").map_err(vm_error)?,
            budget,
            records: BTreeMap::new(),
            failed: false,
        })
    }

    /// Return this codec's null sentinel.
    pub fn null(&self) -> VmValue {
        self.scope.wrap(self.null_lua())
    }

    fn null_lua(&self) -> LuaValue {
        LuaValue::UserData(self.null.clone())
    }

    /// Attach a host-only JSON shape marker to a table.
    pub fn mark(&mut self, table: &Table, shape: JsonShape) -> Result<(), ValueError> {
        self.run(|this| {
            this.budget.container(1)?;
            if !this.scope.same_lua(&table.lua()) {
                return Err(ValueError("foreign VM table"));
            }
            if this.shape(table)?.is_some_and(|current| current != shape) {
                return Err(ValueError("conflicting JSON shape"));
            }
            let marker = match shape {
                JsonShape::Object => &this.object,
                JsonShape::Array => &this.array,
            };
            table
                .set_metatable(Some(marker.clone()))
                .map_err(|error| this.vm_error(error))
        })
    }

    /// Decode a value according to its GraphQL type and storage representation.
    #[cfg(test)]
    pub fn decode(
        &mut self,
        ir: &MockIr,
        ty: &TypeRef,
        value: &VmValue,
    ) -> Result<Value, ValueError> {
        self.decode_hint(ir, Hint::typed(ty), value)
    }

    pub(crate) fn decode_output(
        &mut self,
        ir: &MockIr,
        ty: &TypeRef,
        value: &VmValue,
    ) -> Result<(Value, Provenance), ValueError> {
        self.decode_hint_with_provenance(ir, Hint::typed(ty), value)
    }

    /// Decode the record payload used by create and update operations.
    pub fn decode_record(
        &mut self,
        ir: &MockIr,
        model: ModelId,
        value: &VmValue,
    ) -> Result<Value, ValueError> {
        if ir.models().get(model.0).is_none() {
            self.fail();
            return Err(ValueError("unknown model"));
        }
        self.decode_hint(ir, Hint::Record(model), value)
    }

    /// Decode schema-independent JSON; an ambiguous empty table requires an explicit constructor.
    pub fn decode_json(&mut self, ir: &MockIr, value: &VmValue) -> Result<Value, ValueError> {
        self.decode_hint(ir, Hint::Json(None), value)
    }

    /// Copy a Rust value into the VM while preserving explicit object and array shapes.
    pub fn encode(&mut self, value: &Value) -> Result<VmValue, ValueError> {
        self.run(|this| this.encode_value(value, 1))
            .map(|value| self.scope.wrap(value))
    }

    /// Copy a model record into the VM and include its concrete typename for abstract output.
    pub fn encode_record(
        &mut self,
        ir: &MockIr,
        record: &Arc<Record>,
    ) -> Result<VmValue, ValueError> {
        self.run(|this| this.record(ir, record, 1))
            .map(|value| self.scope.wrap(value))
    }

    fn vm_error(&self, error: luaur::Error) -> ValueError {
        observed_vm_error(error, &self.budget.work)
    }

    /// Mark this codec unusable after a host or VM failure.
    pub fn fail(&mut self) {
        self.failed = true;
        self.scope.fail();
    }

    fn decode_hint(
        &mut self,
        ir: &MockIr,
        hint: Hint<'_>,
        value: &VmValue,
    ) -> Result<Value, ValueError> {
        self.decode_hint_with_provenance(ir, hint, value)
            .map(|(value, _)| value)
    }

    fn decode_hint_with_provenance(
        &mut self,
        ir: &MockIr,
        hint: Hint<'_>,
        value: &VmValue,
    ) -> Result<(Value, Provenance), ValueError> {
        self.run(|this| {
            let value = this.scope.value(value)?;
            let mut scan = scan::Scan::default();
            let value = this.decode_value(ir, hint, value, 1, &mut scan)?;
            Ok((value, scan.provenance))
        })
    }

    fn shape(&self, table: &Table) -> Result<Option<JsonShape>, ValueError> {
        let Some(marker) = table.metatable() else {
            return Ok(None);
        };
        if marker.to_pointer() == self.object.to_pointer() {
            Ok(Some(JsonShape::Object))
        } else if marker.to_pointer() == self.array.to_pointer() {
            Ok(Some(JsonShape::Array))
        } else {
            Err(ValueError("foreign metatable"))
        }
    }

    fn ready(&self) -> Result<(), ValueError> {
        if self.failed || self.scope.check().is_err() {
            Err(ValueError("conversion failure prevents execution"))
        } else {
            Ok(())
        }
    }

    /// Check for any conversion failure captured during VM execution before committing.
    pub fn check(&self) -> Result<(), ValueError> {
        self.ready()
    }

    fn run<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, ValueError>,
    ) -> Result<T, ValueError> {
        self.ready()?;
        // luaur's direct allocation API can report its memory limit by unwinding.
        // Any such failure makes this codec and its VM unusable.
        let result = catch_unwind(AssertUnwindSafe(|| operation(self)))
            .unwrap_or(Err(ValueError("VM allocation failed")));
        if result.is_err() {
            self.fail();
        }
        result
    }
}

fn observed_vm_error(error: luaur::Error, work: &crate::value::work::Work) -> ValueError {
    if matches!(error, luaur::Error::MemoryError(_)) {
        work.clone()
            .fail_local(crate::limits::request::LocalLimit::VmMemory);
    }
    vm_error(error)
}

fn vm_error(_: luaur::Error) -> ValueError {
    ValueError("VM value conversion failed")
}

impl Drop for Codec {
    fn drop(&mut self) {
        // Invalidate detached scopes so they cannot be reused after the codec and VM handles disappear.
        self.scope.fail();
    }
}
