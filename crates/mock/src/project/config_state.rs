//! Decode configuration state against its declared shape, preserving empty containers and missing nullable fields.

use std::collections::BTreeMap;

use chigiri::core::{Object, Value};
use luaur::Value as LuaValue;

use super::ProjectError;
use super::config_decode::{Decoder, invalid, limit};
use crate::hooks::{HookSettings, Registration, StateContract, StateFields, StateType};
use crate::value::{MAX_ITEMS, MAX_STRING_BYTES};

impl Decoder {
    pub(super) fn hooks(&mut self, value: LuaValue) -> Result<HookSettings, ProjectError> {
        let [request, operation, field] =
            self.fields(value, ["request", "operation", "field"], 2)?;
        Ok(HookSettings {
            request: self.registration(request)?,
            operation: self.registration(operation)?,
            field: self.registration(field)?,
        })
    }

    fn registration(&mut self, value: LuaValue) -> Result<Registration, ProjectError> {
        let [before, after] = self.fields(value, ["before", "after"], 3)?;
        let mut boolean = |value| {
            self.budget.container(4).map_err(|_| limit())?;
            match value {
                LuaValue::Boolean(value) => Ok(value),
                _ => Err(invalid()),
            }
        };
        Ok(Registration {
            before: boolean(before)?,
            after: boolean(after)?,
        })
    }

    pub(super) fn state(&mut self, value: LuaValue) -> Result<StateContract, ProjectError> {
        let [fields, initial] = self.fields(value, ["fields", "initial"], 2)?;
        let fields = self.state_fields(fields, 3)?;
        let initial = self.state_object(&fields, initial, 3)?;
        StateContract::checked(fields, &Value::Object(initial), &mut self.budget).map_err(|error| {
            if error.0 == "value resource limit" {
                limit()
            } else {
                ProjectError::new("state", "invalid state declaration or initial value")
            }
        })
    }

    fn state_fields(&mut self, value: LuaValue, depth: usize) -> Result<StateFields, ProjectError> {
        let table = self.table(value, depth)?;
        let mut result = StateFields::new();
        let entries = self
            .state_scan
            .entries(&table, &mut self.budget, depth)
            .map_err(|_| limit())?;
        for (key, value) in entries.iter() {
            let name = self.state_string(key.clone(), depth + 1)?;
            result.insert(name, self.state_kind(value.clone(), depth + 1)?);
        }
        Ok(result)
    }

    fn state_kind(&mut self, value: LuaValue, depth: usize) -> Result<StateType, ProjectError> {
        let table = self.table(value, depth)?;
        let kind = self.state_string(table.raw_get("kind").map_err(|_| invalid())?, depth + 1)?;
        let entries = self
            .state_scan
            .entries(&table, &mut self.budget, depth)
            .map_err(|_| limit())?;
        let member = match kind.as_str() {
            "object" => Some("fields"),
            "list" | "nullable" => Some("of"),
            "string" | "boolean" | "integer" | "number" => None,
            _ => return Err(invalid()),
        };
        if entries.len() != 1 + usize::from(member.is_some()) {
            return Err(invalid());
        }
        for (key, _) in entries.iter() {
            let key = self.state_string(key.clone(), depth + 1)?;
            if key != "kind" && Some(key.as_str()) != member {
                return Err(invalid());
            }
        }
        match kind.as_str() {
            "string" | "boolean" | "integer" | "number" => Ok(match kind.as_str() {
                "string" => StateType::String,
                "boolean" => StateType::Boolean,
                "integer" => StateType::Integer,
                _ => StateType::Number,
            }),
            "object" => {
                let fields = table.raw_get("fields").map_err(|_| invalid())?;
                Ok(StateType::Object {
                    fields: self.state_fields(fields, depth + 1)?,
                })
            }
            "list" | "nullable" => {
                let of = table.raw_get("of").map_err(|_| invalid())?;
                let of = Box::new(self.state_kind(of, depth + 1)?);
                Ok(if kind == "list" {
                    StateType::List { of }
                } else {
                    StateType::Nullable { of }
                })
            }
            _ => Err(invalid()),
        }
    }

    fn state_object(
        &mut self,
        fields: &StateFields,
        value: LuaValue,
        depth: usize,
    ) -> Result<Object, ProjectError> {
        let table = self.table(value, depth)?;
        let mut result = Object::new();
        let entries = self
            .state_scan
            .entries(&table, &mut self.budget, depth)
            .map_err(|_| limit())?;
        for (key, value) in entries.iter() {
            let name = self.state_string(key.clone(), depth + 1)?;
            let ty = fields.get(&name).ok_or_else(invalid)?;
            result.insert(name, self.state_value(ty, value.clone(), depth + 1)?);
        }
        Ok(result)
    }

    fn state_value(
        &mut self,
        ty: &StateType,
        value: LuaValue,
        depth: usize,
    ) -> Result<Value, ProjectError> {
        if let StateType::Nullable { of } = ty {
            return self.state_value(of, value, depth);
        }
        if let StateType::Object { fields } = ty {
            return self.state_object(fields, value, depth).map(Value::Object);
        }
        if let StateType::List { of } = ty {
            let table = self.table(value, depth)?;
            let mut items = BTreeMap::new();
            let entries = self
                .state_scan
                .entries(&table, &mut self.budget, depth)
                .map_err(|_| limit())?;
            for (key, value) in entries.iter() {
                self.budget.items(items.len() + 1).map_err(|_| limit())?;
                let LuaValue::Integer(index) = key else {
                    return Err(invalid());
                };
                if !(1..=MAX_ITEMS as i64).contains(index) {
                    return Err(invalid());
                }
                items.insert(*index, self.state_value(of, value.clone(), depth + 1)?);
            }
            if items.keys().copied().ne(1..=items.len() as i64) {
                return Err(invalid());
            }
            return Ok(Value::List(items.into_values().collect()));
        }
        if matches!(ty, StateType::String) {
            return self.state_string(value, depth).map(Value::String);
        }
        let result = match (ty, value) {
            (StateType::Boolean, LuaValue::Boolean(value)) => Value::Boolean(value),
            (StateType::Integer | StateType::Number, value) => {
                crate::value::numbers::from_luau(&value).map_err(|_| invalid())?
            }
            _ => return Err(invalid()),
        };
        self.budget.node(&result, depth).map_err(|_| limit())?;
        Ok(result)
    }

    fn state_string(&mut self, value: LuaValue, depth: usize) -> Result<String, ProjectError> {
        // The shared string reader charges aggregate usage and enforces the byte limit before copying.
        self.string_limited(value, depth, MAX_STRING_BYTES)
    }
}
