//! Schema and introspection lookup for field validation using a precomputed catalog.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::language::ast::{DirectiveLocation, Field, OperationType, TypeRef, TypeRefKind};
use crate::schema::introspection::model::{FieldDescriptor, TypeWrapper};
use crate::schema::{SemanticSchema, TypeKind};

/// Normalizes a field return type for merge validation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct OutputType<'a> {
    pub(super) wrappers: Vec<u8>,
    pub(super) named: &'a str,
    pub(super) is_leaf: bool,
}

impl OutputType<'_> {
    pub(super) fn has_same_shape(&self, other: &Self) -> bool {
        self.wrappers == other.wrappers
            && self.is_leaf == other.is_leaf
            && (!self.is_leaf || self.named == other.named)
    }
}

pub(super) struct FieldArgumentRule {
    pub(super) arguments: BTreeSet<String>,
    pub(super) required: Vec<String>,
}

/// Builds argument rules from a field referenced by the document.
pub(super) struct FieldArgumentCatalog<'schema> {
    schema: &'schema SemanticSchema,
    rules: BTreeMap<&'schema str, BTreeMap<String, Option<FieldArgumentRule>>>,
}

impl<'schema> FieldArgumentCatalog<'schema> {
    pub(super) fn new(schema: &'schema SemanticSchema) -> Self {
        Self {
            schema,
            rules: BTreeMap::new(),
        }
    }

    pub(super) fn get(&mut self, parent: &str, field: &str) -> Option<&FieldArgumentRule> {
        let parent = self
            .schema
            .type_by_name(parent)
            .map(|ty| ty.name())
            .or_else(|| crate::schema::introspection::type_by_name(parent).map(|ty| ty.name))?;
        self.rules
            .entry(parent)
            .or_default()
            .entry(field.to_owned())
            .or_insert_with(|| argument_rule_for(self.schema, parent, field))
            .as_ref()
    }
}

pub(super) struct PossibleObjects<'schema>(&'schema SemanticSchema);

impl<'schema> PossibleObjects<'schema> {
    pub(super) fn new(schema: &'schema SemanticSchema) -> Self {
        Self(schema)
    }

    pub(super) fn get(&self, name: &str) -> Arc<[Arc<str>]> {
        self.0
            .possible_objects(name)
            .cloned()
            .or_else(|| introspection_possible_objects(name))
            .unwrap_or_default()
    }

    pub(super) fn intersect(&self, current: &[Arc<str>], target: &str) -> Option<Arc<[Arc<str>]>> {
        let target = self
            .0
            .possible_objects(target)
            .cloned()
            .or_else(|| introspection_possible_objects(target))?;
        let mut output = Vec::new();
        let (mut left, mut right) = (0, 0);
        while left < current.len() && right < target.len() {
            match current[left].cmp(&target[right]) {
                std::cmp::Ordering::Less => left += 1,
                std::cmp::Ordering::Greater => right += 1,
                std::cmp::Ordering::Equal => {
                    output.push(current[left].clone());
                    left += 1;
                    right += 1;
                }
            }
        }
        Some(Arc::from(output))
    }
}

fn introspection_possible_objects(name: &str) -> Option<Arc<[Arc<str>]>> {
    crate::schema::introspection::type_by_name(name)
        .filter(|descriptor| !descriptor.fields.is_empty())
        .map(|descriptor| Arc::from([Arc::from(descriptor.name)]))
}

fn argument_rule_for(
    schema: &SemanticSchema,
    parent: &str,
    field: &str,
) -> Option<FieldArgumentRule> {
    if is_query_root(schema, parent) {
        match field {
            "__schema" => return Some(argument_rule(std::iter::empty())),
            "__type" => return Some(argument_rule(std::iter::once(("name", true)))),
            _ => {}
        }
    }
    if let Some(definition) = schema.type_by_name(parent).and_then(|ty| ty.field(field)) {
        return Some(argument_rule(definition.arguments.iter().map(|argument| {
            (
                argument.name.value.as_str(),
                required(&argument.ty, argument.default_value.is_some()),
            )
        })));
    }
    let definition = descriptor_field(parent, field)?;
    Some(argument_rule(definition.arguments.iter().map(|argument| {
        (
            argument.name,
            argument.ty.wrappers.first() == Some(&TypeWrapper::NonNull)
                && argument.default_value.is_none(),
        )
    })))
}

fn argument_rule<'a>(arguments: impl Iterator<Item = (&'a str, bool)>) -> FieldArgumentRule {
    let mut names = BTreeSet::new();
    let mut required = Vec::new();
    for (name, is_required) in arguments {
        names.insert(name.to_owned());
        if is_required {
            required.push(name.to_owned());
        }
    }
    FieldArgumentRule {
        arguments: names,
        required,
    }
}

pub(super) fn operation_location(operation: OperationType) -> DirectiveLocation {
    match operation {
        OperationType::Query => DirectiveLocation::Query,
        OperationType::Mutation => DirectiveLocation::Mutation,
        OperationType::Subscription => DirectiveLocation::Subscription,
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

fn required(reference: &TypeRef, has_default: bool) -> bool {
    matches!(reference.kind, TypeRefKind::NonNull(_)) && !has_default
}

pub(super) fn field_output<'a>(
    schema: &'a SemanticSchema,
    parent: &str,
    field: &Field,
) -> Option<OutputType<'a>> {
    if field.name.value == "__typename" && is_composite(schema, parent) {
        return Some(output(vec![1], "String", true));
    }
    if is_query_root(schema, parent) && field.name.value == "__schema" {
        return Some(output(vec![1], "__Schema", false));
    }
    if is_query_root(schema, parent) && field.name.value == "__type" {
        return Some(output(Vec::new(), "__Type", false));
    }
    if let Some(definition) = schema
        .type_by_name(parent)
        .and_then(|ty| ty.field(&field.name.value))
    {
        return Some(output_from_source(schema, &definition.ty));
    }
    descriptor_field(parent, &field.name.value).map(|definition| OutputType {
        wrappers: definition
            .ty
            .wrappers
            .iter()
            .map(|wrapper| match wrapper {
                TypeWrapper::List => 2,
                TypeWrapper::NonNull => 1,
            })
            .collect(),
        named: definition.ty.named_type,
        is_leaf: !is_composite(schema, definition.ty.named_type),
    })
}

fn output(wrappers: Vec<u8>, named: &str, is_leaf: bool) -> OutputType<'_> {
    OutputType {
        wrappers,
        named,
        is_leaf,
    }
}

fn descriptor_field(parent: &str, field: &str) -> Option<&'static FieldDescriptor> {
    crate::schema::introspection::type_by_name(parent)?
        .fields
        .iter()
        .find(|candidate| candidate.name == field)
}

fn output_from_source<'a>(schema: &SemanticSchema, reference: &'a TypeRef) -> OutputType<'a> {
    let mut wrappers = Vec::new();
    let mut current = reference;
    let named = loop {
        match &current.kind {
            TypeRefKind::Named(name) => break name.value.as_str(),
            TypeRefKind::List(inner) => {
                wrappers.push(2);
                current = inner;
            }
            TypeRefKind::NonNull(inner) => {
                wrappers.push(1);
                current = inner;
            }
        }
    };
    let is_leaf = !is_composite(schema, named);
    OutputType {
        wrappers,
        named,
        is_leaf,
    }
}

pub(super) fn is_composite(schema: &SemanticSchema, name: &str) -> bool {
    schema.type_by_name(name).is_some_and(|ty| {
        matches!(
            ty.kind(),
            TypeKind::Object | TypeKind::Interface | TypeKind::Union
        )
    }) || crate::schema::introspection::type_by_name(name).is_some_and(|ty| !ty.fields.is_empty())
}

pub(super) fn is_concrete_object(schema: &SemanticSchema, name: &str) -> bool {
    schema
        .type_by_name(name)
        .is_some_and(|ty| ty.kind() == TypeKind::Object)
}

fn is_query_root(schema: &SemanticSchema, parent: &str) -> bool {
    schema
        .roots()
        .query()
        .is_some_and(|root| root.type_name == parent)
}
