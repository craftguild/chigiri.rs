//! Describe each model's storage shape, relation ownership, and resolver execution capabilities from the IR.

use super::{ReferenceError, budget::Budget, schema_types::*, schema_values::*};
use crate::{
    api::{self, Effect},
    ir::{FieldId, FieldPlan, MockIr, ModelId, RelationStorage, SlotId, Wrapper},
};

pub(super) fn append(
    doc: &mut SchemaDoc,
    ir: &MockIr,
    budget: &mut Budget,
) -> Result<(), ReferenceError> {
    for (index, model) in ir.models().iter().enumerate() {
        budget.push(
            &mut doc.models,
            ModelDoc {
                id: id("model", index),
                type_id: id("type", model.ty.0),
                description: description(
                    ir,
                    ir.graph().source().semantic().types()[model.ty.0].description(),
                ),
                key_field_id: model.key.map(|key| id("field", key.0)),
                slot_ids: model
                    .slots
                    .values()
                    .map(|slot| id("slot", slot.0))
                    .collect(),
                relation_ids: ir
                    .relations()
                    .iter()
                    .enumerate()
                    .filter(|(_, relation)| {
                        relation.owner.0 == index
                            || relation.target.0 == index && !relation.inverses.is_empty()
                    })
                    .map(|(index, _)| id("relation", index))
                    .collect(),
                method_ids: methods(ir, ModelId(index), Effect::Write),
                api_types: ModelApiTypesDoc {
                    record_slot_ids: model
                        .slots
                        .values()
                        .map(|slot| id("slot", slot.0))
                        .collect(),
                    create_slot_ids: model
                        .slots
                        .values()
                        .map(|slot| id("slot", slot.0))
                        .collect(),
                    update_slot_ids: model
                        .slots
                        .values()
                        .filter(|slot| !model.immutable_slots.contains(slot))
                        .map(|slot| id("slot", slot.0))
                        .collect(),
                    predicate_slot_ids: model
                        .slots
                        .values()
                        .filter(|slot| api::query::scalar(ir, &ir.slots()[slot.0]).is_some())
                        .map(|slot| id("slot", slot.0))
                        .collect(),
                    sort_slot_ids: model
                        .slots
                        .values()
                        .filter(|slot| api::query::scalar(ir, &ir.slots()[slot.0]).is_some())
                        .map(|slot| id("slot", slot.0))
                        .collect(),
                    relation_field_ids: ir
                        .relation_fields(ModelId(index))
                        .map(|field| id("field", field.0))
                        .collect(),
                },
                visibility: if ir.is_private_type(model.ty) {
                    VisibilityDoc::Storage
                } else {
                    VisibilityDoc::Public
                },
                source: ir.graph().types()[model.ty.0]
                    .span
                    .and_then(|span| source_name(ir, span.source_id.index())),
            },
        )?;
    }
    for (index, slot) in ir.slots().iter().enumerate() {
        let model = &ir.models()[slot.model.0];
        let kind = api::query::scalar(ir, slot);
        budget.push(
            &mut doc.slots,
            SlotDoc {
                id: id("slot", index),
                model_id: id("model", slot.model.0),
                field_id: id("field", slot.field.0),
                name: slot.name.to_string(),
                description: declared_slot_description(ir, SlotId(index), slot.field),
                value_type: reference(&slot.ty),
                constraints: constraints(&slot.constraints),
                model_key: model.key == Some(slot.field),
                create_required: !(slot.ty.nullable()
                    || model.auto_id && model.key == Some(slot.field)),
                writable: !model.immutable_slots.contains(&crate::ir::SlotId(index)),
                filter_operator_ids: api::query::OPERATORS
                    .iter()
                    .filter(|op| kind.is_some_and(|kind| op.eligibility.allows(kind)))
                    .map(|op| format!("operator.{}", op.name))
                    .collect(),
                sortable: kind.is_some(),
                visibility: if ir.is_private_field(slot.field) {
                    VisibilityDoc::Storage
                } else {
                    VisibilityDoc::Public
                },
                source: source_name(ir, ir.graph().fields()[slot.field.0].span.source_id.index()),
            },
        )?;
    }
    for (index, relation) in ir.relations().iter().enumerate() {
        let ty = &ir.graph().fields()[relation.field.0].ty;
        let list = ty
            .wrappers
            .iter()
            .position(|wrapper| *wrapper == Wrapper::List);
        budget.push(
            &mut doc.relations,
            RelationDoc {
                id: id("relation", index),
                owner_model_id: id("model", relation.owner.0),
                owner_field_id: id("field", relation.field.0),
                storage_slot_ids: match &relation.storage {
                    RelationStorage::ForeignKey { slots, .. } => {
                        slots.iter().map(|slot| id("slot", slot.0)).collect()
                    }
                    RelationStorage::Implicit { .. } => Vec::new(),
                },
                reference_slot_ids: {
                    let key = match relation.storage {
                        RelationStorage::ForeignKey { key, .. } => key,
                        RelationStorage::Implicit { target_key, .. } => target_key,
                    };
                    ir.keys()[key.0]
                        .slots
                        .iter()
                        .map(|slot| id("slot", slot.0))
                        .collect()
                },
                implicit: matches!(relation.storage, RelationStorage::Implicit { .. }),
                name: relation.name.as_ref().map(ToString::to_string),
                target_model_id: id("model", relation.target.0),
                inverse_field_ids: relation
                    .inverses
                    .iter()
                    .map(|field| id("field", field.0))
                    .collect(),
                many: list.is_some(),
                nullable: ty.nullable(),
                nullable_items: list
                    .map(|index| ty.wrappers.get(index + 1) != Some(&Wrapper::NonNull)),
                unique: relation.unique,
                min_refs: relation.min_refs,
                max_refs: relation.max_refs,
            },
        )?;
    }
    for (name, effect) in [
        ("capability/read", Effect::Read),
        ("capability/write", Effect::Write),
    ] {
        budget.push(
            &mut doc.capabilities,
            CapabilityDoc {
                id: name.into(),
                models: ir
                    .models()
                    .iter()
                    .enumerate()
                    .map(|(index, _)| ModelCapabilityDoc {
                        model_id: id("model", index),
                        method_ids: methods(ir, ModelId(index), effect),
                    })
                    .collect(),
            },
        )?;
    }
    for (index, script) in ir.scripts().iter().enumerate() {
        let field = &ir.graph().fields()[script.field.0];
        let mut scopes = Vec::new();
        if script.role.is_mutation_root() {
            scopes.push(ResolverScopeDoc {
                scope: ResolverScope::MutationRoot,
                capability_id: "capability/write".into(),
            });
        } else if script.role.is_subscription_root() {
            scopes.push(ResolverScopeDoc {
                scope: ResolverScope::SubscriptionSetup,
                capability_id: "capability/read".into(),
            });
        } else if field.parent == ir.graph().query() {
            scopes.push(ResolverScopeDoc {
                scope: ResolverScope::QueryRoot,
                capability_id: "capability/read".into(),
            });
        }
        if script.role.root_as_nested()
            || (!script.role.is_mutation_root()
                && field.parent != ir.graph().query()
                && Some(field.parent) != ir.graph().subscription())
        {
            scopes.push(ResolverScopeDoc {
                scope: ResolverScope::Nested,
                capability_id: "capability/read".into(),
            });
        }
        budget.push(
            &mut doc.resolvers,
            ResolverDoc {
                id: id("resolver", index),
                field_id: id("field", script.field.0),
                coordinate: format!("{}.{}", ir.graph().types()[field.parent.0].name, field.name),
                description: ir
                    .graph()
                    .field_source(script.field)
                    .and_then(|field| description(ir, field.description.as_ref())),
                argument_ids: field
                    .arguments
                    .iter()
                    .map(|input| id("input", input.0))
                    .collect(),
                returns: reference(&field.ty),
                scopes,
            },
        )?;
    }
    Ok(())
}

fn declared_slot_description(ir: &MockIr, slot: SlotId, field: FieldId) -> Option<DescriptionDoc> {
    match ir.field_plan(field) {
        Some(FieldPlan::Stored(declared)) if *declared == slot => ir
            .graph()
            .field_source(field)
            .and_then(|source| description(ir, source.description.as_ref())),
        _ => None,
    }
}

fn source_name(ir: &MockIr, index: usize) -> Option<String> {
    ir.graph()
        .source()
        .sources()
        .get(index)
        .map(|source| source.name().to_owned())
}

fn methods(ir: &MockIr, model: ModelId, effect: Effect) -> Vec<String> {
    api::METHODS
        .iter()
        .filter(|method| {
            method.eligibility.allows(ir, model) && method.effect.allowed(effect == Effect::Write)
        })
        .map(|method| method.id.into())
        .collect()
}
