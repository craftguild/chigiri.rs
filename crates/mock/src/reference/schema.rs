//! Connect source descriptions and the IR type graph through stable IDs.

use super::{ReferenceError, budget::Budget, schema_types::*, schema_values::*};
use crate::ir::{FieldId, FieldPlan, MockIr, TypeId};
use chigiri::core::schema::TypeKind;

pub(super) fn empty(ir: &MockIr) -> SchemaDoc {
    SchemaDoc {
        digest: ir.graph().source().digest().to_hex(),
        vocabulary: crate::schema::VOCABULARY_VERSION.into(),
        description: description(ir, ir.graph().source().semantic().description()),
        query_type_id: id("type", ir.graph().query().0),
        mutation_type_id: ir.graph().mutation().map(|ty| id("type", ty.0)),
        subscription_type_id: ir.graph().subscription().map(|ty| id("type", ty.0)),
        types: Vec::new(),
        fields: Vec::new(),
        inputs: Vec::new(),
        models: Vec::new(),
        slots: Vec::new(),
        relations: Vec::new(),
        resolvers: Vec::new(),
        capabilities: Vec::new(),
    }
}

pub(super) fn append(
    doc: &mut SchemaDoc,
    ir: &MockIr,
    budget: &mut Budget,
) -> Result<(), ReferenceError> {
    let graph = ir.graph();
    let mut input_descriptions = vec![None; graph.inputs().len()];
    for (index, ty) in graph.types().iter().enumerate() {
        let source = &graph.source().semantic().types()[index];
        for input in source.input_fields() {
            let input_id = ty
                .inputs
                .get(input.name.value.as_str())
                .ok_or_else(ReferenceError::contract)?;
            input_descriptions[input_id.0] = input.description.as_ref();
        }
        budget.push(
            &mut doc.types,
            TypeDoc {
                id: id("type", index),
                name: ty.name.to_string(),
                kind: match ty.kind {
                    TypeKind::Scalar => TypeKindDoc::Scalar,
                    TypeKind::Enum => TypeKindDoc::Enum,
                    TypeKind::Object => TypeKindDoc::Object,
                    TypeKind::Interface => TypeKindDoc::Interface,
                    TypeKind::Union => TypeKindDoc::Union,
                    TypeKind::InputObject => TypeKindDoc::InputObject,
                },
                description: description(ir, source.description()),
                model_id: ir
                    .model_for(TypeId(index))
                    .map(|model| id("model", model.0)),
                field_ids: ty
                    .fields
                    .values()
                    .map(|field| id("field", field.0))
                    .collect(),
                input_ids: ty
                    .inputs
                    .values()
                    .map(|input| id("input", input.0))
                    .collect(),
                enum_values: source
                    .enum_values()
                    .iter()
                    .map(|value| EnumValueDoc {
                        name: value.name.value.clone(),
                        description: description(ir, value.description.as_ref()),
                    })
                    .collect(),
                interface_ids: ty.interfaces.iter().map(|ty| id("type", ty.0)).collect(),
                possible_type_ids: ty
                    .possible_types
                    .iter()
                    .map(|ty| id("type", ty.0))
                    .collect(),
                one_of: ty.one_of,
                scalar: ty.scalar.as_ref().map(scalar),
                visibility: if ir.is_private_type(TypeId(index)) {
                    VisibilityDoc::Storage
                } else {
                    VisibilityDoc::Public
                },
            },
        )?;
    }
    for (index, field) in graph.fields().iter().enumerate() {
        let source = graph
            .field_source(FieldId(index))
            .ok_or_else(ReferenceError::contract)?;
        for (input, source) in field.arguments.iter().zip(&source.arguments) {
            input_descriptions[input.0] = source.description.as_ref();
        }
        budget.push(
            &mut doc.fields,
            FieldDoc {
                id: id("field", index),
                parent_type_id: id("type", field.parent.0),
                name: field.name.to_string(),
                description: description(ir, source.description.as_ref()),
                value_type: reference(&field.ty),
                argument_ids: field
                    .arguments
                    .iter()
                    .map(|input| id("input", input.0))
                    .collect(),
                constraints: constraints(&field.constraints),
                role: match ir.field_plan(FieldId(index)) {
                    Some(FieldPlan::Stored(slot)) => FieldRoleDoc::Stored {
                        slot_id: id("slot", slot.0),
                    },
                    Some(FieldPlan::Value) => FieldRoleDoc::Value,
                    Some(FieldPlan::Owner(relation)) => FieldRoleDoc::Owner {
                        relation_id: id("relation", relation.0),
                    },
                    Some(FieldPlan::Inverse { relation }) => FieldRoleDoc::Inverse {
                        relation_id: id("relation", relation.0),
                    },
                    Some(FieldPlan::Script(script)) => FieldRoleDoc::Resolver {
                        resolver_id: id("resolver", script.0),
                    },
                    None => FieldRoleDoc::Declaration,
                },
                visibility: if ir.is_private_field(FieldId(index)) {
                    VisibilityDoc::Storage
                } else {
                    VisibilityDoc::Public
                },
            },
        )?;
    }
    for (index, input) in graph.inputs().iter().enumerate() {
        budget.push(
            &mut doc.inputs,
            InputDoc {
                id: id("input", index),
                name: input.name.to_string(),
                description: description(ir, input_descriptions[index]),
                value_type: reference(&input.ty),
                default: input.default.as_ref().map(literal).transpose()?,
                required_after_coercion: !input.ty.nullable() || input.default.is_some(),
                constraints: constraints(&input.constraints),
            },
        )?;
    }
    Ok(())
}
