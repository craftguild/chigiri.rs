//! Encode hook inputs and decode hook results with one VM conversion budget.

use chigiri::core::{Object, Value};
use luaur::Value as LuaValue;

use super::{Codec, JsonShape, VmValue, hint::Hint};
use crate::hooks::{HookSlot, RequestState};
use crate::ir::MockIr;
use crate::value::ValueError;

impl Codec {
    /// Decode a borrowed hook result without cloning before its nested tables are validated.
    pub(crate) fn readonly_object(&mut self, object: &Object) -> Result<VmValue, ValueError> {
        self.run(|this| {
            this.budget.container(1)?;
            let value = this.object_value(object, 1)?;
            freeze(&value, &this.budget.work)?;
            Ok(value)
        })
        .map(|value| self.scope.wrap(value))
    }

    /// Copy and freeze a phase snapshot so Luau cannot mutate the original Rust value.
    pub(crate) fn readonly(&mut self, value: &Value) -> Result<VmValue, ValueError> {
        self.run(|this| {
            let value =
                this.encode_limited(value, 1, crate::limits::HOOK_OBSERVATION_TEXT_BYTES)?;
            freeze(&value, &this.budget.work)?;
            Ok(value)
        })
        .map(|value| self.scope.wrap(value))
    }

    /// Decode a contract-defined object directly, so an empty table needs no JSON-shape constructor.
    pub(crate) fn decode_object(
        &mut self,
        ir: &MockIr,
        value: &VmValue,
    ) -> Result<Value, ValueError> {
        let decoded = self.decode_hint(ir, Hint::Json(Some(JsonShape::Object)), value)?;
        if matches!(decoded, Value::Object(_)) {
            Ok(decoded)
        } else {
            self.fail();
            Err(ValueError("object required"))
        }
    }

    /// Accept nil or validate the closed result envelope and return its checked state snapshot.
    pub(crate) fn hook_result(
        &mut self,
        ir: &MockIr,
        slot: HookSlot,
        current: &RequestState,
        result: &VmValue,
    ) -> Result<Option<RequestState>, ValueError> {
        self.run(|this| {
            let raw = this.scope.value(result)?;
            if matches!(raw, LuaValue::Nil) {
                return Ok(None);
            }
            if !slot.allows_patch() {
                return Err(ValueError("hook must return nil"));
            }
            let envelope = this.decode_value(
                ir,
                Hint::HookReturn(current.fields()),
                raw,
                1,
                &mut Default::default(),
            )?;
            let Value::Object(envelope) = envelope else {
                return Err(ValueError("hook return object required"));
            };
            if envelope.len() != 1 {
                return Err(ValueError("statePatch required"));
            }
            let patch = envelope
                .get(crate::api::hooks::PATCH)
                .ok_or(ValueError("statePatch required"))?;
            let mut next = current.clone();
            next.apply_patch(slot, patch, &mut this.budget)?;
            Ok(Some(next))
        })
    }
}

fn freeze(value: &LuaValue, work: &crate::value::work::Work) -> Result<(), ValueError> {
    if let LuaValue::Table(table) = value {
        // The table graph was already scanned, so cycles cannot reach this recursive freeze.
        for value in table.clone().pairs::<LuaValue, LuaValue>() {
            freeze(
                &value
                    .map_err(|error| super::observed_vm_error(error, work))?
                    .1,
                work,
            )?;
        }
        table.set_readonly(true);
    }
    Ok(())
}
