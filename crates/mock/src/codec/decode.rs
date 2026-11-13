//! Decode tables by raw iteration, rejecting cycles, holes, invalid keys, and unknown metatables.

use std::collections::BTreeMap;

use chigiri::core::{Object, Value};
use luaur::{LuaString, Table, Value as LuaValue};

use super::hint::{Container, Fields, Hint};
use super::scan::Scan;
use super::{Codec, HostStringKind, JsonShape, PathPart};
use crate::ir::{FieldPlan, MockIr, TypeId};
use crate::schema::MAX_SDL_BYTES;
use crate::value::{MAX_ITEMS, MAX_STRING_BYTES, ValueError};

impl Codec {
    pub(crate) fn bounded_host_string(
        &mut self,
        text: &LuaString,
        max: usize,
        kind: HostStringKind,
    ) -> Result<String, ValueError> {
        let depth = match kind {
            HostStringKind::Key => None,
            HostStringKind::Value => Some(1),
        };
        self.text(text, max, depth)
    }

    pub(super) fn decode_value(
        &mut self,
        ir: &MockIr,
        hint: Hint<'_>,
        value: &LuaValue,
        depth: usize,
        scan: &mut Scan,
    ) -> Result<Value, ValueError> {
        let value = match value {
            LuaValue::Nil => return Err(ValueError("missing value; explicit null required")),
            LuaValue::UserData(value) if value.to_pointer() == self.null.to_pointer() => {
                Value::Null
            }
            LuaValue::Boolean(value) => Value::Boolean(*value),
            LuaValue::Integer(_) | LuaValue::Number(_) => crate::value::numbers::from_luau(value)?,
            LuaValue::String(text) => {
                let max = if matches!(hint, Hint::Typename | Hint::FieldName) {
                    MAX_SDL_BYTES
                } else {
                    MAX_STRING_BYTES
                };
                return self.text(text, max, Some(depth)).map(Value::String);
            }
            LuaValue::Table(table) => {
                self.budget.container(depth)?;
                let pointer = table.to_pointer() as usize;
                if !scan.active_tables.insert(pointer) {
                    return Err(ValueError("cyclic table"));
                }
                if let Some((_, record)) = self.records.get(&pointer) {
                    scan.record(record.clone());
                }
                let result = self.table(ir, hint, table, depth, scan);
                scan.active_tables.remove(&pointer);
                return result;
            }
            _ => return Err(ValueError("unsupported or nonfinite VM value")),
        };
        self.budget.node(&value, depth)?;
        Ok(value)
    }

    fn table(
        &mut self,
        ir: &MockIr,
        hint: Hint<'_>,
        table: &Table,
        depth: usize,
        scan: &mut Scan,
    ) -> Result<Value, ValueError> {
        let marker = self.shape(table)?;
        let entries = scan.entries(table, &mut self.budget, depth)?;
        let container = match hint.container(ir)? {
            Container::Abstract(expected) => self.abstract_fields(ir, expected, table, depth)?,
            Container::Json => match marker {
                Some(JsonShape::Object) => Container::Object(Fields::Json),
                Some(JsonShape::Array) => Container::Array(Hint::Json(None)),
                None => match entries.first() {
                    Some((LuaValue::String(_), _)) => Container::Object(Fields::Json),
                    Some((LuaValue::Integer(_) | LuaValue::Number(_), _)) => {
                        Container::Array(Hint::Json(None))
                    }
                    Some(_) => return Err(ValueError("JSON key must be string or array index")),
                    None => return Err(ValueError("empty JSON table requires explicit shape")),
                },
            },
            other => other,
        };
        match container {
            Container::Array(item) => {
                if marker == Some(JsonShape::Object) {
                    return Err(ValueError("object marker on array"));
                }
                self.array(ir, item, &entries, depth, scan)
            }
            Container::Object(fields) => {
                if marker == Some(JsonShape::Array) {
                    return Err(ValueError("array marker on object"));
                }
                self.object(ir, fields, &entries, depth, scan)
            }
            _ => Err(ValueError("unresolved container shape")),
        }
    }

    fn array(
        &mut self,
        ir: &MockIr,
        hint: Hint<'_>,
        entries: &[(LuaValue, LuaValue)],
        depth: usize,
        scan: &mut Scan,
    ) -> Result<Value, ValueError> {
        let mut items = BTreeMap::new();
        for (key, value) in entries {
            self.budget.items(items.len() + 1)?;
            let index = match key {
                LuaValue::Integer(index) if (1..=MAX_ITEMS as i64).contains(index) => {
                    *index as usize
                }
                LuaValue::Number(index)
                    if index.fract() == 0.0 && (1.0..=MAX_ITEMS as f64).contains(index) =>
                {
                    *index as usize
                }
                _ => return Err(ValueError("invalid array index or mixed keys")),
            };
            scan.enter(PathPart::Index(index - 1));
            let decoded = self.decode_value(ir, hint, value, depth + 1, scan);
            scan.leave();
            items.insert(index, decoded?);
        }
        if items
            .last_key_value()
            .is_some_and(|(last, _)| *last != items.len())
        {
            return Err(ValueError("sparse array or nil hole"));
        }
        Ok(Value::List(items.into_values().collect()))
    }

    fn object(
        &mut self,
        ir: &MockIr,
        fields: Fields<'_>,
        entries: &[(LuaValue, LuaValue)],
        depth: usize,
        scan: &mut Scan,
    ) -> Result<Value, ValueError> {
        let mut object = Object::new();
        for (key, value) in entries {
            let LuaValue::String(key) = key else {
                return Err(ValueError("object string key required"));
            };
            let max = if matches!(
                fields,
                Fields::Json | Fields::State(_) | Fields::HookReturn(_)
            ) {
                MAX_STRING_BYTES
            } else {
                MAX_SDL_BYTES
            };
            let name = self.text(key, max, None)?;
            let hint = field_hint(ir, fields, &name)?;
            scan.enter(PathPart::Field(name.clone()));
            let decoded = self.decode_value(ir, hint, value, depth + 1, scan);
            scan.leave();
            object.insert(name, decoded?);
        }
        Ok(Value::Object(object))
    }

    fn abstract_fields(
        &mut self,
        ir: &MockIr,
        expected: TypeId,
        table: &Table,
        depth: usize,
    ) -> Result<Container<'static>, ValueError> {
        let value: LuaValue = table
            .raw_get("__typename")
            .map_err(|error| self.vm_error(error))?;
        let LuaValue::String(value) = value else {
            return Err(ValueError("abstract output requires __typename"));
        };
        // Charge the abstract-type discriminator as one string copy before resolving it.
        let name = self.text(&value, MAX_SDL_BYTES, Some(depth + 1))?;
        let concrete = ir
            .graph()
            .type_id(&name)
            .ok_or(ValueError("unknown concrete output type"))?;
        if ir.graph().types()[expected.0]
            .possible_types
            .binary_search(&concrete)
            .is_err()
        {
            return Err(ValueError("output type is not a possible member"));
        }
        Ok(Container::Object(
            ir.model_for(concrete)
                .map_or(Fields::Type(concrete), Fields::Record),
        ))
    }

    fn text(
        &mut self,
        text: &LuaString,
        max: usize,
        depth: Option<usize>,
    ) -> Result<String, ValueError> {
        // Measure through Luau before `to_str` allocates the Rust copy, then reserve its budget.
        let bytes: usize = self
            .string_len
            .call(text.clone())
            .map_err(|error| self.vm_error(error))?;
        if bytes > max {
            return Err(ValueError("string or key length"));
        }
        if let Some(depth) = depth {
            self.budget.string_node(bytes, depth)?;
        } else {
            self.budget.key_bytes(bytes)?;
        }
        text.to_str().map_err(|error| self.vm_error(error))
    }
}

fn field_hint<'a>(ir: &'a MockIr, fields: Fields<'a>, name: &str) -> Result<Hint<'a>, ValueError> {
    match fields {
        Fields::State(fields) => fields
            .get(name)
            .map(Hint::State)
            .ok_or(ValueError("unknown state field")),
        Fields::HookReturn(fields) if name == crate::api::hooks::PATCH => {
            Ok(Hint::StateFields(fields))
        }
        Fields::HookReturn(_) => Err(ValueError("unknown hook return field")),
        Fields::Json => Ok(Hint::Json(None)),
        Fields::Query(count) => {
            let specs = if count {
                crate::api::COUNT_OPTIONS
            } else {
                crate::api::FIND_OPTIONS
            };
            let option = specs
                .iter()
                .find(|option| option.name == name)
                .ok_or(ValueError("unknown find option"))?;
            Ok(match option.ty {
                crate::api::OptionType::Where => Hint::Predicate,
                crate::api::OptionType::OrderBy => Hint::OrderKeys,
                crate::api::OptionType::Integer(_) => Hint::Json(None),
            })
        }
        Fields::Predicate => query_hint(crate::api::shape::predicate_field(name)),
        Fields::OrderKey => query_hint(crate::api::shape::order_field(name)),
        Fields::Record(model) => {
            if name == "__typename" {
                return Ok(Hint::Typename);
            }
            let slot = ir.models()[model.0]
                .slots
                .get(name)
                .ok_or(ValueError("unknown or computed record field"))?;
            Ok(Hint::typed(&ir.slots()[slot.0].ty))
        }
        Fields::Type(ty) => {
            let node = &ir.graph().types()[ty.0];
            if node.kind == chigiri::core::schema::TypeKind::InputObject {
                let id = node
                    .inputs
                    .get(name)
                    .ok_or(ValueError("unknown input field"))?;
                return Ok(Hint::typed(&ir.graph().inputs()[id.0].ty));
            }
            if name == "__typename" {
                return Ok(Hint::Typename);
            }
            let id = *node
                .fields
                .get(name)
                .ok_or(ValueError("unknown DTO field"))?;
            if !matches!(ir.field_plan(id), Some(FieldPlan::Value)) {
                return Err(ValueError("computed DTO field supplied"));
            }
            Ok(Hint::typed(&ir.graph().fields()[id.0].ty))
        }
    }
}

fn query_hint(ty: Option<crate::api::shape::QueryType>) -> Result<Hint<'static>, ValueError> {
    use crate::api::shape::QueryType;
    match ty {
        Some(QueryType::Predicates) => Ok(Hint::Predicates),
        Some(QueryType::FieldName) => Ok(Hint::FieldName),
        Some(QueryType::Operand) => Ok(Hint::Operand),
        Some(QueryType::Operator | QueryType::Direction) => Ok(Hint::Json(None)),
        None => Err(ValueError("unknown query key")),
    }
}
