//! Resolves explicit seed snapshots before records and relation indexes are built.

use std::borrow::Cow;
use std::collections::BTreeSet;

use chigiri::core::schema::TypeKind;
use chigiri::core::{Object, Value};

use super::StoreError;
use super::seed_ref_plan::{ObjectPlan, ValuePlan};
use crate::api::model_values::TYPENAME;
use crate::ir::{FieldPlan, MockIr, ModelId, SlotId, TypeRef, Wrapper};
use crate::value::{MAX_DEPTH, ValueBudget, ValueError, ValueValidator};

const MARKER: &str = "$ref";

struct Resolver<'a, 'b> {
    ir: &'a MockIr,
    seed: &'a Object,
    budget: &'b mut ValueBudget,
    path: BTreeSet<(ModelId, usize)>,
}

pub(super) fn expand<'a>(
    ir: &'a MockIr,
    model: ModelId,
    row: &'a Object,
    seed: &'a Object,
    budget: &mut ValueBudget,
) -> Result<Object, StoreError> {
    let plan = {
        let mut resolver = Resolver {
            ir,
            seed,
            budget,
            path: BTreeSet::new(),
        };
        resolver.record(model, row, 1)?
    };
    Ok(plan.reserve(budget, 1)?.materialize())
}

impl<'a> Resolver<'a, '_> {
    fn record(
        &mut self,
        model: ModelId,
        row: &'a Object,
        depth: usize,
    ) -> Result<ObjectPlan<'a>, StoreError> {
        self.depth(depth)?;
        let mut output = ObjectPlan::with_capacity(row.len());
        for (name, value) in row {
            self.budget.work.charge(1 + name.len())?;
            let planned = match self.ir.models()[model.0].slots.get(name).copied() {
                Some(slot) => {
                    let ty = self.ir.slots()[slot.0].ty.clone();
                    self.value(&ty, 0, value, depth + 1)?
                }
                None => ValuePlan::Borrowed(value),
            };
            output.push(Cow::Borrowed(name), planned);
        }
        Ok(output)
    }

    fn value(
        &mut self,
        ty: &TypeRef,
        mut wrapper: usize,
        value: &'a Value,
        depth: usize,
    ) -> Result<ValuePlan<'a>, StoreError> {
        self.depth(depth)?;
        if ty.wrappers.get(wrapper) == Some(&Wrapper::NonNull) {
            wrapper += 1;
        } else if value.is_null() {
            return Ok(ValuePlan::Borrowed(value));
        }
        if ty.wrappers.get(wrapper) == Some(&Wrapper::List) {
            let Value::List(items) = value else {
                return Ok(ValuePlan::Borrowed(value));
            };
            let mut output = Vec::with_capacity(items.len());
            self.budget.work.charge(items.len())?;
            for item in items {
                output.push(self.value(ty, wrapper + 1, item, depth + 1)?);
            }
            return Ok(ValuePlan::List(output));
        }
        let kind = self.ir.graph().types()[ty.named.0].kind;
        if !matches!(
            kind,
            TypeKind::Object | TypeKind::Interface | TypeKind::Union
        ) {
            return Ok(ValuePlan::Borrowed(value));
        }
        let Value::Object(object) = value else {
            return Ok(ValuePlan::Borrowed(value));
        };
        if object.contains_key(MARKER) {
            return self.reference(ty.named, object, depth);
        }
        let mut output = ObjectPlan::with_capacity(object.len());
        let concrete = self.concrete(ty.named, object)?;
        for (name, value) in object {
            if name == TYPENAME {
                output.push(Cow::Borrowed(name), ValuePlan::Borrowed(value));
                continue;
            }
            self.budget.work.charge(1 + name.len())?;
            let planned = match self.ir.graph().types()[concrete.0].fields.get(name) {
                Some(field) => {
                    let field = &self.ir.graph().fields()[field.0];
                    self.value(&field.ty, 0, value, depth + 1)?
                }
                None => ValuePlan::Borrowed(value),
            };
            output.push(Cow::Borrowed(name), planned);
        }
        Ok(ValuePlan::Object(output))
    }

    fn reference(
        &mut self,
        expected: crate::ir::TypeId,
        marker: &'a Object,
        depth: usize,
    ) -> Result<ValuePlan<'a>, StoreError> {
        if marker.len() != 1 {
            return Err(ValueError("seed reference marker must be closed"));
        }
        let Value::Object(reference) = marker.get(MARKER).expect("checked marker") else {
            return Err(ValueError("seed reference requires an object"));
        };
        if reference.len() != 2 {
            return Err(ValueError("seed reference must contain model and key"));
        }
        let model_name = reference
            .get("model")
            .and_then(Value::as_str)
            .ok_or(ValueError("seed reference model requires a string"))?;
        let Value::Object(key) = reference
            .get("key")
            .ok_or(ValueError("seed reference key missing"))?
        else {
            return Err(ValueError("seed reference key requires an object"));
        };
        let model = self
            .ir
            .graph()
            .type_id(model_name)
            .and_then(|ty| self.ir.model_for(ty))
            .ok_or(ValueError("unknown seed reference model"))?;
        self.compatible(expected, model)?;
        let slots = self.unique_key(model, key)?;
        for slot in &slots {
            let name = &self.ir.slots()[slot.0].name;
            let value = key.get(name).expect("matched key fields");
            ValueValidator::new(self.ir, self.budget).slot(*slot, value)?;
        }
        let (index, row) = self.lookup(model, &slots, key)?;
        if !self.path.insert((model, index)) {
            return Err(ValueError("cyclic seed reference expansion"));
        }
        let result = self.project(expected, model, row, depth + 1);
        self.path.remove(&(model, index));
        result.map(ValuePlan::Object)
    }

    fn unique_key(&self, model: ModelId, key: &Object) -> Result<Vec<SlotId>, StoreError> {
        if key.is_empty() {
            return Err(ValueError("seed reference key cannot be empty"));
        }
        let names: BTreeSet<&str> = key.iter().map(|(name, _)| name).collect();
        let definition = &self.ir.models()[model.0];
        let identity = definition
            .key
            .and_then(|field| {
                definition
                    .slots
                    .get(&self.ir.graph().fields()[field.0].name)
            })
            .copied()
            .map(|slot| vec![slot]);
        identity
            .into_iter()
            .chain(
                self.ir
                    .keys()
                    .iter()
                    .filter(move |candidate| candidate.model == model)
                    .map(|candidate| candidate.slots.clone()),
            )
            .find(|slots| {
                slots.len() == names.len()
                    && slots
                        .iter()
                        .all(|slot| names.contains(self.ir.slots()[slot.0].name.as_ref()))
            })
            .ok_or(ValueError("seed reference key is not declared unique"))
    }

    fn lookup(
        &mut self,
        model: ModelId,
        slots: &[SlotId],
        key: &Object,
    ) -> Result<(usize, &'a Object), StoreError> {
        let name = &self.ir.graph().types()[self.ir.models()[model.0].ty.0].name;
        let Value::List(rows) = self
            .seed
            .get(name.as_ref())
            .ok_or(ValueError("seed reference target model missing"))?
        else {
            return Err(ValueError("seed reference target model requires an array"));
        };
        let mut found = None;
        for (index, value) in rows.iter().enumerate() {
            self.budget.work.charge(1 + slots.len())?;
            let Value::Object(row) = value else { continue };
            if slots.iter().all(|slot| {
                let field = self.ir.slots()[slot.0].name.as_ref();
                row.get(field) == key.get(field)
            }) {
                if found.is_some() {
                    return Err(ValueError("ambiguous seed reference target"));
                }
                found = Some((index, row));
            }
        }
        found.ok_or(ValueError("missing seed reference target"))
    }

    fn project(
        &mut self,
        expected: crate::ir::TypeId,
        model: ModelId,
        row: &'a Object,
        depth: usize,
    ) -> Result<ObjectPlan<'a>, StoreError> {
        self.depth(depth)?;
        let actual = self.ir.models()[model.0].ty;
        let mut output = ObjectPlan::with_capacity(row.len() + usize::from(expected != actual));
        if expected != actual {
            output.push(
                Cow::Borrowed(TYPENAME),
                ValuePlan::String(self.ir.graph().types()[actual.0].name.as_ref()),
            );
        }
        for (name, value) in row {
            self.budget.work.charge(1 + name.len())?;
            let Some(field) = self.ir.graph().types()[actual.0].fields.get(name) else {
                continue;
            };
            if !matches!(
                self.ir.field_plan(*field),
                Some(FieldPlan::Stored(_) | FieldPlan::Value)
            ) {
                continue;
            }
            let field = &self.ir.graph().fields()[field.0];
            output.push(
                Cow::Borrowed(name),
                self.value(&field.ty, 0, value, depth + 1)?,
            );
        }
        Ok(output)
    }

    fn compatible(&self, expected: crate::ir::TypeId, model: ModelId) -> Result<(), StoreError> {
        let actual = self.ir.models()[model.0].ty;
        let node = &self.ir.graph().types()[expected.0];
        if expected == actual || node.possible_types.binary_search(&actual).is_ok() {
            Ok(())
        } else {
            Err(ValueError("seed reference model type mismatch"))
        }
    }

    fn concrete(
        &self,
        expected: crate::ir::TypeId,
        object: &Object,
    ) -> Result<crate::ir::TypeId, StoreError> {
        if self.ir.graph().types()[expected.0].kind == TypeKind::Object {
            return Ok(expected);
        }
        object
            .get(TYPENAME)
            .and_then(Value::as_str)
            .and_then(|name| self.ir.graph().type_id(name))
            .ok_or(ValueError(
                "stored abstract value requires concrete typename",
            ))
    }

    fn depth(&mut self, depth: usize) -> Result<(), StoreError> {
        self.budget.work.charge(1)?;
        if depth > MAX_DEPTH {
            Err(ValueError("seed reference depth limit"))
        } else {
            Ok(())
        }
    }
}
