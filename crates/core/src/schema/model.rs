//! Composed types and schema roots, preserving source order and name-based lookup.

use crate::Span;
use crate::language::ast::{
    Description, Directive, EnumValueDefinition, FieldDefinition, InputValueDefinition, Name,
    OperationType,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::SemanticDirectiveDefinition;

/// Origin of a semantic element: built-in, implicit, or source-defined.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaOrigin {
    /// Element injected from the GraphQL specification.
    BuiltIn,
    /// The root was selected by the default type-name convention.
    Implicit,
    /// Element derived from a type-system definition.
    Definition(Span),
    /// Added elements from type-system extension.
    Extension(Span),
}

impl SchemaOrigin {
    /// Returns the span for a source-defined element.
    pub const fn span(self) -> Option<Span> {
        match self {
            Self::Definition(span) | Self::Extension(span) => Some(span),
            Self::BuiltIn | Self::Implicit => None,
        }
    }
}

/// Semantic GraphQL named type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypeKind {
    /// Scalar type.
    Scalar,
    /// Object type.
    Object,
    /// Interface type.
    Interface,
    /// Union type.
    Union,
    /// Enum type.
    Enum,
    /// Input-object type.
    InputObject,
}

/// Named type merged from its definition and extensions in source order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticType {
    pub(super) name: String,
    pub(super) name_span: Option<Span>,
    pub(super) kind: TypeKind,
    pub(super) description: Option<Description>,
    pub(super) directives: Vec<Directive>,
    pub(super) directive_origins: Vec<SchemaOrigin>,
    pub(super) origins: Vec<SchemaOrigin>,
    pub(super) implements_interfaces: Vec<Name>,
    pub(super) implemented_interface_index: BTreeSet<String>,
    pub(super) fields: Vec<FieldDefinition>,
    pub(super) union_members: Vec<Name>,
    pub(super) union_member_index: BTreeSet<String>,
    pub(super) enum_values: Vec<EnumValueDefinition>,
    pub(super) input_fields: Vec<InputValueDefinition>,
    pub(super) field_index: BTreeMap<String, usize>,
    pub(super) field_argument_index: BTreeMap<String, BTreeMap<String, usize>>,
    pub(super) enum_value_index: BTreeMap<String, usize>,
    pub(super) input_field_index: BTreeMap<String, usize>,
}

impl SemanticType {
    /// Return the type name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the name-token span for a user-defined type, or `None` for built-ins.
    pub const fn name_span(&self) -> Option<Span> {
        self.name_span
    }

    /// Return the type kind.
    pub const fn kind(&self) -> TypeKind {
        self.kind
    }

    /// Return the definition description.
    pub const fn description(&self) -> Option<&Description> {
        self.description.as_ref()
    }

    /// Returns definition and extension origins in document/source order.
    pub fn directives(&self) -> &[Directive] {
        &self.directives
    }

    /// Returns directive origins in the same order as `directives`.
    pub fn directive_origins(&self) -> &[SchemaOrigin] {
        &self.directive_origins
    }

    /// Returns definition and extension origins in document/source order.
    pub fn origins(&self) -> &[SchemaOrigin] {
        &self.origins
    }

    /// Returns implemented interface names in source order.
    pub fn implemented_interfaces(&self) -> &[Name] {
        &self.implements_interfaces
    }

    /// Return whether this object explicitly implements the named interface.
    pub fn implements_interface(&self, name: &str) -> bool {
        self.implemented_interface_index.contains(name)
    }

    /// Returns object or interface fields in source order.
    pub fn fields(&self) -> &[FieldDefinition] {
        &self.fields
    }

    /// Looks up an object or interface field by name.
    pub fn field(&self, name: &str) -> Option<&FieldDefinition> {
        self.field_index
            .get(name)
            .and_then(|index| self.fields.get(*index))
    }

    /// Look up an object or interface field argument by name.
    pub fn field_argument(
        &self,
        field_name: &str,
        argument_name: &str,
    ) -> Option<&InputValueDefinition> {
        let argument_index = self
            .field_argument_index
            .get(field_name)?
            .get(argument_name)?;
        self.field(field_name)?.arguments.get(*argument_index)
    }

    /// Return union member names in source order.
    pub fn union_members(&self) -> &[Name] {
        &self.union_members
    }

    /// Return whether the named object is a union member.
    pub fn has_union_member(&self, name: &str) -> bool {
        self.union_member_index.contains(name)
    }

    /// Returns enum values in source order.
    pub fn enum_values(&self) -> &[EnumValueDefinition] {
        &self.enum_values
    }

    /// Looks up an enum value by name.
    pub fn enum_value(&self, name: &str) -> Option<&EnumValueDefinition> {
        self.enum_value_index
            .get(name)
            .and_then(|index| self.enum_values.get(*index))
    }

    /// Returns input-object fields in source order.
    pub fn input_fields(&self) -> &[InputValueDefinition] {
        &self.input_fields
    }

    /// Looks up an input-object field by name.
    pub fn input_field(&self, name: &str) -> Option<&InputValueDefinition> {
        self.input_field_index
            .get(name)
            .and_then(|index| self.input_fields.get(*index))
    }

    pub(super) fn append_fields(&mut self, additions: &[FieldDefinition]) {
        let start = self.fields.len();
        self.fields.extend(additions.iter().cloned());
        for (position, field) in self.fields.iter().enumerate().skip(start) {
            if self.field_index.contains_key(&field.name.value) {
                continue;
            }
            self.field_index.insert(field.name.value.clone(), position);
            let arguments = self
                .field_argument_index
                .entry(field.name.value.clone())
                .or_default();
            for (argument_position, argument) in field.arguments.iter().enumerate() {
                arguments
                    .entry(argument.name.value.clone())
                    .or_insert(argument_position);
            }
        }
    }

    pub(super) fn append_implemented_interfaces(&mut self, additions: &[Name]) {
        self.implements_interfaces.extend(additions.iter().cloned());
        self.implemented_interface_index
            .extend(additions.iter().map(|name| name.value.clone()));
    }

    pub(super) fn append_union_members(&mut self, additions: &[Name]) {
        self.union_members.extend(additions.iter().cloned());
        self.union_member_index
            .extend(additions.iter().map(|name| name.value.clone()));
    }

    pub(super) fn append_enum_values(&mut self, additions: &[EnumValueDefinition]) {
        let start = self.enum_values.len();
        self.enum_values.extend(additions.iter().cloned());
        for (position, value) in self.enum_values.iter().enumerate().skip(start) {
            self.enum_value_index
                .entry(value.name.value.clone())
                .or_insert(position);
        }
    }

    pub(super) fn append_input_fields(&mut self, additions: &[InputValueDefinition]) {
        let start = self.input_fields.len();
        self.input_fields.extend(additions.iter().cloned());
        for (position, field) in self.input_fields.iter().enumerate().skip(start) {
            self.input_field_index
                .entry(field.name.value.clone())
                .or_insert(position);
        }
    }
}

/// Mapping from operation types to root object types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootOperation {
    /// Operation type.
    pub operation_type: OperationType,
    /// The root object type name.
    pub type_name: String,
    /// Origin of this root mapping.
    pub origin: SchemaOrigin,
}

/// Root operation mappings for a schema.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OperationRoots {
    pub(super) query: Option<RootOperation>,
    pub(super) mutation: Option<RootOperation>,
    pub(super) subscription: Option<RootOperation>,
}

impl OperationRoots {
    /// Returns the query root, if defined.
    pub const fn query(&self) -> Option<&RootOperation> {
        self.query.as_ref()
    }

    /// Returns the mutation root, if defined.
    pub const fn mutation(&self) -> Option<&RootOperation> {
        self.mutation.as_ref()
    }

    /// Returns the subscription root, if defined.
    pub const fn subscription(&self) -> Option<&RootOperation> {
        self.subscription.as_ref()
    }
}

/// The semantic schema is published only when composition and built-in injection succeed.
#[derive(Clone, Debug)]
pub struct SemanticSchema {
    pub(super) types: Vec<SemanticType>,
    pub(super) type_index: BTreeMap<String, usize>,
    pub(super) directives: Vec<SemanticDirectiveDefinition>,
    pub(super) directive_index: BTreeMap<String, usize>,
    pub(super) roots: OperationRoots,
    pub(super) description: Option<Description>,
    pub(super) schema_directives: Vec<Directive>,
    pub(super) schema_origins: Vec<SchemaOrigin>,
    pub(super) possible_object_index: BTreeMap<String, Arc<[Arc<str>]>>,
}

impl SemanticSchema {
    /// Returns named types with built-ins first, then user definitions in source order.
    pub fn types(&self) -> &[SemanticType] {
        &self.types
    }

    /// Looks up a named type.
    pub fn type_by_name(&self, name: &str) -> Option<&SemanticType> {
        self.type_index
            .get(name)
            .and_then(|index| self.types.get(*index))
    }

    /// Returns directive definitions with built-ins first, then user definitions in source order.
    pub fn directive_definitions(&self) -> &[SemanticDirectiveDefinition] {
        &self.directives
    }

    /// Looks up a directive definition by name.
    pub fn directive_definition(&self, name: &str) -> Option<&SemanticDirectiveDefinition> {
        self.directive_index
            .get(name)
            .and_then(|index| self.directives.get(*index))
    }

    /// Returns the root operation mapping.
    pub const fn roots(&self) -> &OperationRoots {
        &self.roots
    }

    /// Returns the explicit schema definition's description, if present.
    pub const fn description(&self) -> Option<&Description> {
        self.description.as_ref()
    }

    /// Returns schema-definition and extension directives in source order.
    pub fn schema_directives(&self) -> &[Directive] {
        &self.schema_directives
    }

    /// Returns explicit schema definitions and extensions in source order.
    pub fn schema_origins(&self) -> &[SchemaOrigin] {
        &self.schema_origins
    }

    /// Returns object names that may occur at runtime for a composite type.
    pub(crate) fn possible_objects(&self, name: &str) -> Option<&Arc<[Arc<str>]>> {
        self.possible_object_index.get(name)
    }

    pub(super) fn rebuild_possible_object_index(&mut self) {
        let mut possible = BTreeMap::<String, BTreeSet<Arc<str>>>::new();
        let names: BTreeMap<&str, Arc<str>> = self
            .types
            .iter()
            .map(|ty| (ty.name.as_str(), Arc::from(ty.name.as_str())))
            .collect();
        for ty in &self.types {
            match ty.kind {
                TypeKind::Object => {
                    possible.insert(
                        ty.name.clone(),
                        BTreeSet::from([Arc::clone(&names[ty.name.as_str()])]),
                    );
                }
                TypeKind::Interface => {
                    possible.insert(ty.name.clone(), BTreeSet::new());
                }
                TypeKind::Union => {
                    possible.insert(
                        ty.name.clone(),
                        ty.union_members
                            .iter()
                            .map(|member| Arc::clone(&names[member.value.as_str()]))
                            .collect(),
                    );
                }
                TypeKind::Scalar | TypeKind::Enum | TypeKind::InputObject => {}
            }
        }
        for object in self.types.iter().filter(|ty| ty.kind == TypeKind::Object) {
            for interface in &object.implements_interfaces {
                if let Some(objects) = possible.get_mut(&interface.value) {
                    objects.insert(Arc::clone(&names[object.name.as_str()]));
                }
            }
        }
        self.possible_object_index = possible
            .into_iter()
            .map(|(name, objects)| {
                let objects = objects.into_iter().collect::<Vec<_>>();
                (name, Arc::from(objects))
            })
            .collect();
    }
}
