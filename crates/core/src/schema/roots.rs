//! Merges explicit and implicit schema roots and validates the GraphQL root contract.

use crate::language::ast::{
    OperationType, RootOperationTypeDefinition, SchemaDefinition, SchemaExtension,
};
use std::collections::BTreeSet;

use super::composer::Composer;
use super::{RootOperation, SchemaOrigin, TypeKind};

pub(super) fn add_schema_definition(composer: &mut Composer, definition: &SchemaDefinition) {
    if composer.explicit_schema {
        composer.report(
            super::SCHEMA_DUPLICATE_DEFINITION,
            "A schema definition may appear only once",
            definition.span,
        );
        return;
    }
    composer.explicit_schema = true;
    composer.schema.description = definition.description.clone();
    composer
        .schema
        .schema_origins
        .push(SchemaOrigin::Definition(definition.span));
    composer
        .schema
        .schema_directives
        .extend(definition.directives.clone());
    for root in &definition.root_operations {
        add_root(composer, root, SchemaOrigin::Definition(root.span));
        if !composer.can_continue() {
            return;
        }
    }
}

pub(super) fn infer_implicit_roots(composer: &mut Composer) {
    if composer.explicit_schema {
        return;
    }
    for (operation_type, type_name) in [
        (OperationType::Query, "Query"),
        (OperationType::Mutation, "Mutation"),
        (OperationType::Subscription, "Subscription"),
    ] {
        if composer.schema.type_index.contains_key(type_name) {
            set_root(
                composer,
                RootOperation {
                    operation_type,
                    type_name: type_name.to_owned(),
                    origin: SchemaOrigin::Implicit,
                },
            );
        }
    }
}

pub(super) fn apply_schema_extension(composer: &mut Composer, extension: &SchemaExtension) {
    let has_implicit_schema = composer.schema.roots.query.is_some()
        || composer.schema.roots.mutation.is_some()
        || composer.schema.roots.subscription.is_some();
    if !composer.explicit_schema && !has_implicit_schema {
        composer.report(
            super::SCHEMA_EXTENSION_TARGET_MISSING,
            "Cannot extend a schema that is not defined",
            extension.span,
        );
        return;
    }
    composer
        .schema
        .schema_origins
        .push(SchemaOrigin::Extension(extension.span));
    composer
        .schema
        .schema_directives
        .extend(extension.directives.clone());
    for root in &extension.root_operations {
        add_root(composer, root, SchemaOrigin::Extension(root.span));
        if !composer.can_continue() {
            return;
        }
    }
}

fn add_root(
    composer: &mut Composer,
    definition: &RootOperationTypeDefinition,
    origin: SchemaOrigin,
) {
    let root = RootOperation {
        operation_type: definition.operation_type.value,
        type_name: definition.named_type.value.clone(),
        origin,
    };
    if root_slot(&composer.schema.roots, root.operation_type).is_some() {
        composer.report(
            super::SCHEMA_DUPLICATE_ROOT,
            "Duplicate schema root operation mapping",
            definition.operation_type.span,
        );
    } else {
        set_root(composer, root);
    }
}

fn set_root(composer: &mut Composer, root: RootOperation) {
    match root.operation_type {
        OperationType::Query => composer.schema.roots.query = Some(root),
        OperationType::Mutation => composer.schema.roots.mutation = Some(root),
        OperationType::Subscription => composer.schema.roots.subscription = Some(root),
    }
}

fn root_slot(
    roots: &super::OperationRoots,
    operation_type: OperationType,
) -> Option<&RootOperation> {
    match operation_type {
        OperationType::Query => roots.query.as_ref(),
        OperationType::Mutation => roots.mutation.as_ref(),
        OperationType::Subscription => roots.subscription.as_ref(),
    }
}

pub(super) fn validate_roots(composer: &mut Composer) {
    if composer.schema.roots.query.is_none() {
        composer.report_without_span(
            super::SCHEMA_QUERY_ROOT_MISSING,
            "A schema must define a query root object",
        );
        if !composer.can_continue() {
            return;
        }
    }
    let roots = [
        composer.schema.roots.query.clone(),
        composer.schema.roots.mutation.clone(),
        composer.schema.roots.subscription.clone(),
    ];
    for root in roots.iter().flatten() {
        validate_root_type(composer, root);
        if !composer.can_continue() {
            return;
        }
    }
    validate_distinct_roots(composer, roots.iter().flatten());
}

fn validate_root_type(composer: &mut Composer, root: &RootOperation) {
    let Some(named_type) = composer.schema.type_by_name(&root.type_name) else {
        composer.report_origin(
            super::SCHEMA_ROOT_TYPE_MISSING,
            format!("Root type `{}` is not defined", root.type_name),
            root.origin,
        );
        return;
    };
    if named_type.kind() != TypeKind::Object {
        composer.report_origin(
            super::SCHEMA_ROOT_TYPE_NOT_OBJECT,
            format!("Root type `{}` must be an object type", root.type_name),
            root.origin,
        );
    }
}

fn validate_distinct_roots<'a>(
    composer: &mut Composer,
    roots: impl IntoIterator<Item = &'a RootOperation>,
) {
    let mut seen = BTreeSet::new();
    for root in roots {
        if !seen.insert(root.type_name.as_str()) {
            composer.report_origin(
                super::SCHEMA_ROOT_TYPES_NOT_DISTINCT,
                format!(
                    "Root type `{}` is used by multiple operations",
                    root.type_name
                ),
                root.origin,
            );
            if !composer.can_continue() {
                return;
            }
        }
    }
}
