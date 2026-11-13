//! Catalogs expected input types for fields, directives, and introspection arguments.

use crate::language::ast::{OperationType, TypeRef, TypeRefKind};
use crate::schema::SemanticSchema;
use crate::schema::introspection::model::FieldDescriptor;

use super::types::InputType;

pub(super) struct Expected<'a> {
    pub(super) ty: InputType<'a>,
    pub(super) has_default: bool,
}

/// Borrows only schema coordinates referenced by the document without copying the schema.
pub(super) struct Catalog<'schema> {
    schema: &'schema SemanticSchema,
}

impl<'schema> Catalog<'schema> {
    pub(super) const fn new(schema: &'schema SemanticSchema) -> Self {
        Self { schema }
    }

    pub(super) fn field_output(&self, parent: &str, field: &str) -> Option<&'schema str> {
        if let Some(definition) = self
            .schema
            .type_by_name(parent)
            .and_then(|ty| ty.field(field))
        {
            return Some(named_type(&definition.ty));
        }
        if self.is_query_root(parent) {
            match field {
                "__schema" => return Some("__Schema"),
                "__type" => return Some("__Type"),
                _ => {}
            }
        }
        descriptor_field(parent, field).map(|definition| definition.ty.named_type)
    }

    pub(super) fn field_argument(
        &self,
        parent: &str,
        field: &str,
        argument: &str,
    ) -> Option<Expected<'schema>> {
        if let Some(definition) = self
            .schema
            .type_by_name(parent)
            .and_then(|ty| ty.field_argument(field, argument))
        {
            return Some(Expected {
                ty: InputType::from_ast(&definition.ty),
                has_default: definition.default_value.is_some(),
            });
        }
        if self.is_query_root(parent) && field == "__type" && argument == "name" {
            return Some(Expected {
                ty: InputType::non_null_named("String"),
                has_default: false,
            });
        }
        let definition = descriptor_field(parent, field)?
            .arguments
            .iter()
            .find(|definition| definition.name == argument)?;
        Some(Expected {
            ty: InputType::from_descriptor(definition.ty),
            has_default: definition.default_value.is_some(),
        })
    }

    pub(super) fn directive_argument(
        &self,
        directive: &str,
        argument: &str,
    ) -> Option<Expected<'schema>> {
        let definition = self
            .schema
            .directive_definition(directive)?
            .argument(argument)?;
        Some(Expected {
            ty: InputType::from_semantic(&definition.ty),
            has_default: definition.default_value.is_some(),
        })
    }

    fn is_query_root(&self, parent: &str) -> bool {
        self.schema
            .roots()
            .query()
            .is_some_and(|root| root.type_name == parent)
    }
}

fn descriptor_field(parent: &str, field: &str) -> Option<&'static FieldDescriptor> {
    crate::schema::introspection::type_by_name(parent)?
        .fields
        .iter()
        .find(|definition| definition.name == field)
}

fn named_type(reference: &TypeRef) -> &str {
    let mut current = reference;
    loop {
        match &current.kind {
            TypeRefKind::Named(name) => return &name.value,
            TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => current = inner,
        }
    }
}

pub(super) fn root_name(schema: &SemanticSchema, operation: OperationType) -> Option<&str> {
    match operation {
        OperationType::Query => schema.roots().query(),
        OperationType::Mutation => schema.roots().mutation(),
        OperationType::Subscription => schema.roots().subscription(),
    }
    .map(|root| root.type_name.as_str())
}
