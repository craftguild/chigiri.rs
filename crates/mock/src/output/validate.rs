//! Validate wrappers, concrete types, DTO shape, and stored records under one shared budget.

mod record;

use std::collections::BTreeMap;
use std::sync::Arc;

use chigiri::core::schema::TypeKind;
use chigiri::core::{Object, Value};

use super::{ObjectData, OutputObject, OutputValue};
use crate::codec::{PathPart, Provenance, RecordOrigin};
use crate::ir::{Constraints, FieldId, FieldNode, FieldPlan, TypeId, Wrapper};
use crate::store::Snapshot;
use crate::value::{ValueBudget, ValueError, ValueValidator, constraints};

pub(super) struct Validator<'a> {
    pub(super) snapshot: &'a Snapshot,
    pub(super) budget: &'a mut ValueBudget,
    provenance: Option<&'a Provenance>,
    pub(super) path: Vec<PathPart>,
    pub(super) failure_member: Option<FieldId>,
}

#[derive(Clone, Copy)]
pub(super) struct Mode {
    pub(super) outer: bool,
    pub(super) inline: bool,
    pub(super) stored: bool,
}

impl Validator<'_> {
    pub(super) fn new<'a>(
        snapshot: &'a Snapshot,
        budget: &'a mut ValueBudget,
        provenance: Option<&'a Provenance>,
    ) -> Validator<'a> {
        Validator {
            snapshot,
            budget,
            provenance,
            path: Vec::new(),
            failure_member: None,
        }
    }

    pub(super) fn field(
        &mut self,
        field: &FieldNode,
        value: &Value,
        depth: usize,
    ) -> Result<OutputValue, ValueError> {
        let ir = self.snapshot.ir();
        let id = ir.graph().types()[field.parent.0].fields[&field.name];
        let inline = matches!(
            ir.field_plan(id),
            Some(FieldPlan::Stored(_) | FieldPlan::Value)
        );
        self.typed(
            field.ty.named,
            &field.ty.wrappers,
            Some(&field.constraints),
            value,
            depth,
            Mode {
                outer: true,
                inline,
                stored: false,
            },
        )
    }

    pub(super) fn stored_field(
        &mut self,
        field: &FieldNode,
        value: &Value,
        depth: usize,
    ) -> Result<OutputValue, ValueError> {
        self.typed(
            field.ty.named,
            &field.ty.wrappers,
            Some(&field.constraints),
            value,
            depth,
            Mode {
                outer: true,
                inline: true,
                stored: true,
            },
        )
    }

    pub(super) fn typed(
        &mut self,
        named: TypeId,
        mut wrappers: &[Wrapper],
        bounds: Option<&Constraints>,
        value: &Value,
        depth: usize,
        mode: Mode,
    ) -> Result<OutputValue, ValueError> {
        self.budget.node(value, depth)?;
        if wrappers.first() == Some(&Wrapper::NonNull) {
            if value.is_null() {
                return Err(ValueError("null for non-null output"));
            }
            wrappers = &wrappers[1..];
        } else if value.is_null() {
            return Ok(OutputValue::Null);
        }
        if wrappers.first() == Some(&Wrapper::List) {
            let Value::List(items) = value else {
                return Err(ValueError("output list required"));
            };
            self.budget.items(items.len())?;
            if mode.outer
                && let Some(bounds) = bounds
            {
                crate::value::conditions::list(bounds, items.len(), self.budget)?;
            }
            let mut result = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                self.path.push(PathPart::Index(index));
                let validated = self.typed(
                    named,
                    &wrappers[1..],
                    bounds,
                    item,
                    depth + 1,
                    Mode {
                        outer: false,
                        ..mode
                    },
                );
                self.path.pop();
                result.push(validated?);
            }
            return Ok(OutputValue::List(result));
        }
        if let Some(bounds) = bounds {
            if mode.outer {
                constraints(bounds, value, self.budget)?;
            } else {
                crate::value::conditions::leaf(bounds, value, self.budget)?;
            }
        }
        let ir = self.snapshot.ir();
        let node = &ir.graph().types()[named.0];
        match node.kind {
            TypeKind::Scalar | TypeKind::Enum => {
                ValueValidator::new(ir, self.budget).leaf(named, value, depth, false)?;
                Ok(OutputValue::Scalar(value.clone()))
            }
            TypeKind::Object | TypeKind::Interface | TypeKind::Union => {
                let Value::Object(object) = value else {
                    return Err(ValueError("output object required"));
                };
                let concrete = self.concrete(named, object, depth, mode.stored)?;
                let model = ir.model_for(concrete);
                let origin = self
                    .provenance
                    .and_then(|provenance| provenance.record(&self.path))
                    .cloned();
                let data = match (model, origin) {
                    _ if mode.stored => super::stored::project(self, concrete, object, depth)?,
                    (Some(model), Some(RecordOrigin::Live(origin))) => {
                        self.provenance_record(model, object, origin, depth)?
                    }
                    (Some(model), Some(RecordOrigin::Captured(origin))) => {
                        self.captured_record(model, object, origin, depth)?
                    }
                    (Some(model), None) if !mode.inline && self.provenance.is_some() => {
                        self.unprovenanced_record(model, object, depth)?
                    }
                    (Some(model), None) if !mode.inline => self.record(model, object, depth)?,
                    _ => self.dto(concrete, object, depth)?,
                };
                Ok(OutputValue::Object(OutputObject {
                    snapshot: self.snapshot.clone(),
                    ty: concrete,
                    data,
                }))
            }
            _ => Err(ValueError("output type required")),
        }
    }

    fn concrete(
        &mut self,
        expected: TypeId,
        object: &Object,
        depth: usize,
        stored: bool,
    ) -> Result<TypeId, ValueError> {
        let graph = self.snapshot.ir().graph();
        let node = &graph.types()[expected.0];
        let Some(value) = object.get("__typename") else {
            return if node.kind == TypeKind::Object {
                Ok(expected)
            } else {
                Err(ValueError("abstract output requires __typename"))
            };
        };
        self.budget.key("__typename")?;
        self.budget.node(value, depth + 1)?;
        let concrete = value
            .as_str()
            .and_then(|name| graph.type_id(name))
            .ok_or(ValueError("unknown concrete output type"))?;
        if !stored && self.snapshot.ir().is_private_type(concrete) {
            return Err(ValueError("private concrete output type"));
        }
        if graph.types()[concrete.0].kind != TypeKind::Object
            || node.possible_types.binary_search(&concrete).is_err()
        {
            return Err(ValueError("output type is not a possible member"));
        }
        Ok(concrete)
    }

    fn dto(&mut self, ty: TypeId, object: &Object, depth: usize) -> Result<ObjectData, ValueError> {
        let ir = self.snapshot.ir();
        let node = &ir.graph().types()[ty.0];
        let mut fields = BTreeMap::new();
        for (name, value) in object {
            if name == "__typename" {
                continue;
            }
            self.budget.key(name)?;
            let id = *node
                .fields
                .get(name)
                .ok_or(ValueError("unknown output DTO field"))?;
            if ir.is_private_field(id) {
                self.failure_member.get_or_insert(id);
                return Err(ValueError("private output DTO field"));
            }
            if !matches!(
                ir.field_plan(id),
                Some(FieldPlan::Value | FieldPlan::Stored(_))
            ) {
                self.failure_member.get_or_insert(id);
                return Err(ValueError("computed output DTO field supplied"));
            }
            self.path.push(PathPart::Field(name.to_owned()));
            let validated = self.field(&ir.graph().fields()[id.0], value, depth + 1);
            self.path.pop();
            if validated.is_err() {
                self.failure_member.get_or_insert(id);
            }
            fields.insert(id, validated?);
        }
        for id in ir.required_value_fields(ty) {
            if !ir.is_private_field(*id) && !fields.contains_key(id) {
                self.failure_member.get_or_insert(*id);
                return Err(ValueError("required output DTO field missing"));
            }
        }
        Ok(ObjectData::Dto(Arc::new(fields)))
    }
}
