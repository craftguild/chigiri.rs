//! Self-contained schema type and resolver topics.

use super::{ReferenceDocument, ReferenceError, topic_types::*};
use chigiri_host::PortableRelativePath;

pub(super) fn types(doc: &ReferenceDocument) -> Result<Vec<TypeTopicDoc>, ReferenceError> {
    let Some(schema) = &doc.schema else {
        return Ok(Vec::new());
    };
    schema
        .types
        .iter()
        .filter(|ty| ty.visibility == super::schema_types::VisibilityDoc::Public)
        .map(|ty| {
            Ok(TypeTopicDoc {
                id: ty.id.clone(),
                name: ty.name.clone(),
                kind: super::topic::serde_name(&ty.kind)?,
                description: ty.description.as_ref().map(|value| value.text.clone()),
                model: ty.model_id.is_some(),
                fields: ty
                    .field_ids
                    .iter()
                    .filter(|id| {
                        schema.fields.iter().any(|field| {
                            &field.id == *id
                                && field.visibility == super::schema_types::VisibilityDoc::Public
                        })
                    })
                    .map(|id| field(schema, id))
                    .collect::<Result<_, _>>()?,
                inputs: ty
                    .input_ids
                    .iter()
                    .map(|id| input(schema, id))
                    .collect::<Result<_, _>>()?,
                enum_values: ty
                    .enum_values
                    .iter()
                    .map(|value| EnumValueTopicDoc {
                        name: value.name.clone(),
                        description: value.description.as_ref().map(|value| value.text.clone()),
                    })
                    .collect(),
                interfaces: type_names(schema, &ty.interface_ids)?,
                possible_types: public_type_names(schema, &ty.possible_type_ids)?,
                one_of: ty.one_of,
                scalar: ty.scalar.clone(),
            })
        })
        .collect()
}

pub(super) fn resolvers(doc: &ReferenceDocument) -> Result<Vec<ResolverTopicDoc>, ReferenceError> {
    let Some(schema) = &doc.schema else {
        return Ok(Vec::new());
    };
    schema
        .resolvers
        .iter()
        .map(|resolver| {
            let field = schema
                .fields
                .iter()
                .find(|field| field.id == resolver.field_id)
                .ok_or_else(ReferenceError::contract)?;
            let parent = schema
                .types
                .iter()
                .find(|ty| ty.id == field.parent_type_id)
                .ok_or_else(ReferenceError::contract)?;
            Ok(ResolverTopicDoc {
                id: resolver.id.clone(),
                coordinate: resolver.coordinate.clone(),
                description: resolver
                    .description
                    .as_ref()
                    .map(|value| value.text.clone()),
                module_path: resolver_path(schema, parent, &field.name),
                arguments: resolver
                    .argument_ids
                    .iter()
                    .map(|id| input(schema, id))
                    .collect::<Result<_, _>>()?,
                returns: super::topic::type_reference(schema, &resolver.returns)?,
                subscription_source: doc
                    .events
                    .types
                    .iter()
                    .flat_map(|event| &event.destinations)
                    .find(|source| source.coordinate == resolver.coordinate)
                    .cloned(),
                scopes: resolver
                    .scopes
                    .iter()
                    .map(|scope| resolver_scope(doc, schema, scope))
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect()
}

fn resolver_scope(
    doc: &ReferenceDocument,
    schema: &super::schema_types::SchemaDoc,
    scope: &super::schema_types::ResolverScopeDoc,
) -> Result<ResolverScopeTopicDoc, ReferenceError> {
    let capability = schema
        .capabilities
        .iter()
        .find(|value| value.id == scope.capability_id)
        .ok_or_else(ReferenceError::contract)?;
    Ok(ResolverScopeTopicDoc {
        scope: super::topic::serde_name(&scope.scope)?,
        models: capability
            .models
            .iter()
            .map(|model| {
                Ok(ModelMethodsTopicDoc {
                    model: super::topic::model_name(schema, &model.model_id)?,
                    methods: model
                        .method_ids
                        .iter()
                        .map(|id| super::topic::method_name(doc, id))
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect::<Result<_, ReferenceError>>()?,
    })
}

fn field(
    schema: &super::schema_types::SchemaDoc,
    id: &str,
) -> Result<TypeFieldTopicDoc, ReferenceError> {
    let field = schema
        .fields
        .iter()
        .find(|field| field.id == id)
        .ok_or_else(ReferenceError::contract)?;
    Ok(TypeFieldTopicDoc {
        name: field.name.clone(),
        description: field.description.as_ref().map(|value| value.text.clone()),
        value_type: super::topic::type_reference(schema, &field.value_type)?,
        arguments: field
            .argument_ids
            .iter()
            .map(|id| input(schema, id))
            .collect::<Result<_, _>>()?,
        constraints: field.constraints.clone(),
        role: field_role(schema, &field.role)?,
    })
}

fn field_role(
    schema: &super::schema_types::SchemaDoc,
    role: &super::schema_types::FieldRoleDoc,
) -> Result<TypeFieldRoleTopicDoc, ReferenceError> {
    use super::schema_types::FieldRoleDoc;
    match role {
        FieldRoleDoc::Stored { slot_id } => {
            let slot = slot(schema, slot_id)?;
            Ok(TypeFieldRoleTopicDoc::Stored {
                model: super::topic::model_name(schema, &slot.model_id)?,
                slot: slot.name.clone(),
                model_key: slot.model_key,
            })
        }
        FieldRoleDoc::Value => Ok(TypeFieldRoleTopicDoc::Value),
        FieldRoleDoc::Owner { relation_id } => Ok(TypeFieldRoleTopicDoc::Owner {
            relation: relation(schema, relation_id)?,
        }),
        FieldRoleDoc::Inverse { relation_id } => Ok(TypeFieldRoleTopicDoc::Inverse {
            relation: relation(schema, relation_id)?,
        }),
        FieldRoleDoc::Resolver { resolver_id } => {
            let resolver = schema
                .resolvers
                .iter()
                .find(|resolver| &resolver.id == resolver_id)
                .ok_or_else(ReferenceError::contract)?;
            let field = schema
                .fields
                .iter()
                .find(|field| field.id == resolver.field_id)
                .ok_or_else(ReferenceError::contract)?;
            let parent = schema
                .types
                .iter()
                .find(|ty| ty.id == field.parent_type_id)
                .ok_or_else(ReferenceError::contract)?;
            Ok(TypeFieldRoleTopicDoc::Resolver {
                coordinate: resolver.coordinate.clone(),
                module_path: resolver_path(schema, parent, &field.name),
            })
        }
        FieldRoleDoc::Declaration => Ok(TypeFieldRoleTopicDoc::Declaration),
    }
}

fn relation(
    schema: &super::schema_types::SchemaDoc,
    id: &str,
) -> Result<FieldRelationTopicDoc, ReferenceError> {
    let relation = schema
        .relations
        .iter()
        .find(|relation| relation.id == id)
        .ok_or_else(ReferenceError::contract)?;
    Ok(FieldRelationTopicDoc {
        name: relation.name.clone(),
        owner_model: super::topic::model_name(schema, &relation.owner_model_id)?,
        owner_field: field_name(schema, &relation.owner_field_id)?,
        target_model: super::topic::model_name(schema, &relation.target_model_id)?,
        storage_keys: relation
            .storage_slot_ids
            .iter()
            .map(|id| slot(schema, id).map(|slot| slot.name.clone()))
            .collect::<Result<_, _>>()?,
        reference_keys: relation
            .reference_slot_ids
            .iter()
            .map(|id| slot(schema, id).map(|slot| slot.name.clone()))
            .collect::<Result<_, _>>()?,
        inverse_fields: relation
            .inverse_field_ids
            .iter()
            .map(|id| field_name(schema, id))
            .collect::<Result<_, _>>()?,
        implicit: relation.implicit,
        many: relation.many,
        nullable: relation.nullable,
        nullable_items: relation.nullable_items,
        unique: relation.unique,
        min_refs: relation.min_refs,
        max_refs: relation.max_refs,
    })
}

fn slot<'a>(
    schema: &'a super::schema_types::SchemaDoc,
    id: &str,
) -> Result<&'a super::schema_types::SlotDoc, ReferenceError> {
    schema
        .slots
        .iter()
        .find(|slot| slot.id == id)
        .ok_or_else(ReferenceError::contract)
}

fn field_name(schema: &super::schema_types::SchemaDoc, id: &str) -> Result<String, ReferenceError> {
    schema
        .fields
        .iter()
        .find(|field| field.id == id)
        .map(|field| field.name.clone())
        .ok_or_else(ReferenceError::contract)
}

fn input(
    schema: &super::schema_types::SchemaDoc,
    id: &str,
) -> Result<InputTopicDoc, ReferenceError> {
    let input = schema
        .inputs
        .iter()
        .find(|input| input.id == id)
        .ok_or_else(ReferenceError::contract)?;
    Ok(InputTopicDoc {
        name: input.name.clone(),
        description: input.description.as_ref().map(|value| value.text.clone()),
        value_type: super::topic::type_reference(schema, &input.value_type)?,
        default: input.default.clone(),
        required_after_coercion: input.required_after_coercion,
        constraints: input.constraints.clone(),
    })
}

fn type_names(
    schema: &super::schema_types::SchemaDoc,
    ids: &[String],
) -> Result<Vec<String>, ReferenceError> {
    ids.iter()
        .map(|id| {
            schema
                .types
                .iter()
                .find(|ty| &ty.id == id)
                .map(|ty| ty.name.clone())
                .ok_or_else(ReferenceError::contract)
        })
        .collect()
}

fn public_type_names(
    schema: &super::schema_types::SchemaDoc,
    ids: &[String],
) -> Result<Vec<String>, ReferenceError> {
    ids.iter()
        .filter_map(|id| match schema.types.iter().find(|ty| &ty.id == id) {
            Some(ty) if ty.visibility == super::schema_types::VisibilityDoc::Public => {
                Some(Ok(ty.name.clone()))
            }
            Some(_) => None,
            None => Some(Err(ReferenceError::contract())),
        })
        .collect()
}

fn resolver_path(
    schema: &super::schema_types::SchemaDoc,
    parent: &super::schema_types::TypeDoc,
    field: &str,
) -> PortableRelativePath {
    let path = if parent.id == schema.query_type_id {
        format!("resolvers/query/{field}.luau")
    } else if schema.mutation_type_id.as_deref() == Some(parent.id.as_str()) {
        format!("resolvers/mutation/{field}.luau")
    } else if schema.subscription_type_id.as_deref() == Some(parent.id.as_str()) {
        format!("resolvers/subscription/{field}.luau")
    } else {
        format!("resolvers/objects/{}/{field}.luau", parent.name)
    };
    PortableRelativePath::new(&path).expect("GraphQL names form portable resolver paths")
}
