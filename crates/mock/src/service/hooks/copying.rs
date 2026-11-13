//! Share node, depth, and byte limits while copying observation values.

use crate::limits::HOOK_OBSERVATION_TEXT_BYTES as TEXT_BYTES;
use crate::value::{ValueBudget, ValueError};
use chigiri::core::Object;
use chigiri::{PathSegment, Value};

pub(super) struct Copy<'a>(pub &'a mut ValueBudget);
impl Copy<'_> {
    pub fn text(&mut self, value: &str, depth: usize) -> Result<Value, ValueError> {
        if value.len() > TEXT_BYTES {
            return Err(ValueError("observation string limit"));
        }
        self.0.string_node(value.len(), depth)?;
        Ok(Value::String(value.to_owned()))
    }
    pub fn value(&mut self, value: &Value, depth: usize) -> Result<Value, ValueError> {
        match value {
            Value::String(value) => self.text(value, depth),
            Value::Object(value) => self.object(value, depth),
            Value::List(values) => {
                self.0.container(depth)?;
                self.0.items(values.len())?;
                values
                    .iter()
                    .map(|value| self.value(value, depth + 1))
                    .collect::<Result<Vec<_>, _>>()
                    .map(Value::List)
            }
            _ => {
                self.0.node(value, depth)?;
                Ok(value.clone())
            }
        }
    }
    pub fn object(&mut self, object: &Object, depth: usize) -> Result<Value, ValueError> {
        self.0.container(depth)?;
        let mut result = Object::new();
        for (name, value) in object {
            self.0.key(name)?;
            result.insert(name, self.value(value, depth + 1)?);
        }
        Ok(Value::Object(result))
    }
    pub fn shape(
        &mut self,
        alias: &str,
        values: Vec<Value>,
        depth: usize,
    ) -> Result<Value, ValueError> {
        let spec = crate::api::hooks::SNAPSHOTS
            .iter()
            .find(|spec| spec.alias == alias)
            .ok_or(ValueError("unknown snapshot type"))?;
        if spec.fields.len() != values.len() {
            return Err(ValueError("snapshot shape mismatch"));
        }
        self.0.container(depth)?;
        let mut result = Object::new();
        for ((name, _), value) in spec.fields.iter().zip(values) {
            self.0.key(name)?;
            result.insert(*name, value);
        }
        Ok(Value::Object(result))
    }
    pub fn path(&mut self, path: &[PathSegment], depth: usize) -> Result<Value, ValueError> {
        self.0.container(depth)?;
        self.0.items(path.len())?;
        let values = path
            .iter()
            .map(|part| match part {
                PathSegment::Field(name) => self.text(name, depth + 1),
                PathSegment::Index(index) => self.value(&Value::Int(*index as i128), depth + 1),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Value::List(values))
    }
}
