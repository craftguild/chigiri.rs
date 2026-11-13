//! Add type-system definitions to the semantic model in source order.

use super::composer::Composer;
use super::{SchemaOrigin, SemanticDirectiveDefinition, SemanticType, TypeKind};
use crate::language::ast::{
    DirectiveDefinition, EnumTypeDefinition, InputObjectTypeDefinition, InterfaceTypeDefinition,
    ObjectTypeDefinition, ScalarTypeDefinition, TypeSystemDefinition, UnionTypeDefinition,
};

pub(super) fn add_definition(composer: &mut Composer, definition: &TypeSystemDefinition) {
    match definition {
        TypeSystemDefinition::Schema(schema) => {
            super::roots::add_schema_definition(composer, schema)
        }
        TypeSystemDefinition::Scalar(scalar) => add_scalar(composer, scalar),
        TypeSystemDefinition::Object(object) => add_object(composer, object),
        TypeSystemDefinition::Interface(interface) => add_interface(composer, interface),
        TypeSystemDefinition::Union(union) => add_union(composer, union),
        TypeSystemDefinition::Enum(enumeration) => add_enum(composer, enumeration),
        TypeSystemDefinition::InputObject(input) => add_input_object(composer, input),
        TypeSystemDefinition::Directive(directive) => add_directive(composer, directive),
        _ => unreachable!("The extension is separated by the collection phase."),
    }
}

fn add_scalar(composer: &mut Composer, definition: &ScalarTypeDefinition) {
    add_type(
        composer,
        base_type(
            &definition.name.value,
            definition.name.span,
            TypeKind::Scalar,
            definition.description.clone(),
            definition.directives.clone(),
            definition.span,
        ),
        definition.name.span,
    );
}

fn add_object(composer: &mut Composer, definition: &ObjectTypeDefinition) {
    super::members::check_fields(composer, &definition.fields);
    if !composer.can_continue() {
        return;
    }
    let mut semantic = base_type(
        &definition.name.value,
        definition.name.span,
        TypeKind::Object,
        definition.description.clone(),
        definition.directives.clone(),
        definition.span,
    );
    semantic.append_implemented_interfaces(&definition.implements_interfaces);
    semantic.append_fields(&definition.fields);
    add_type(composer, semantic, definition.name.span);
}

fn add_interface(composer: &mut Composer, definition: &InterfaceTypeDefinition) {
    super::members::check_fields(composer, &definition.fields);
    if !composer.can_continue() {
        return;
    }
    let mut semantic = base_type(
        &definition.name.value,
        definition.name.span,
        TypeKind::Interface,
        definition.description.clone(),
        definition.directives.clone(),
        definition.span,
    );
    semantic.append_implemented_interfaces(&definition.implements_interfaces);
    semantic.append_fields(&definition.fields);
    add_type(composer, semantic, definition.name.span);
}

fn add_union(composer: &mut Composer, definition: &UnionTypeDefinition) {
    let mut semantic = base_type(
        &definition.name.value,
        definition.name.span,
        TypeKind::Union,
        definition.description.clone(),
        definition.directives.clone(),
        definition.span,
    );
    semantic.append_union_members(&definition.members);
    add_type(composer, semantic, definition.name.span);
}

fn add_enum(composer: &mut Composer, definition: &EnumTypeDefinition) {
    super::members::check_enum_values(composer, &definition.values);
    if !composer.can_continue() {
        return;
    }
    let mut semantic = base_type(
        &definition.name.value,
        definition.name.span,
        TypeKind::Enum,
        definition.description.clone(),
        definition.directives.clone(),
        definition.span,
    );
    semantic.append_enum_values(&definition.values);
    add_type(composer, semantic, definition.name.span);
}

fn add_input_object(composer: &mut Composer, definition: &InputObjectTypeDefinition) {
    super::members::check_input_values(composer, &definition.fields, "input field");
    if !composer.can_continue() {
        return;
    }
    let mut semantic = base_type(
        &definition.name.value,
        definition.name.span,
        TypeKind::InputObject,
        definition.description.clone(),
        definition.directives.clone(),
        definition.span,
    );
    semantic.append_input_fields(&definition.fields);
    add_type(composer, semantic, definition.name.span);
}

fn add_directive(composer: &mut Composer, definition: &DirectiveDefinition) {
    if !super::members::check_directive_definition(composer, definition) {
        return;
    }
    if let Some(equivalent) = super::builtin_imports::is_equivalent(definition, &composer.schema) {
        if equivalent {
            return;
        }
        composer.report(
            super::SCHEMA_DUPLICATE_DIRECTIVE_DEFINITION,
            format!(
                "Built-in directive definition '@{}' conflicts with GraphQL September 2025",
                definition.name.value
            ),
            definition.name.span,
        );
        return;
    }
    if composer
        .schema
        .directive_index
        .contains_key(&definition.name.value)
    {
        composer.report(
            super::SCHEMA_DUPLICATE_DIRECTIVE_DEFINITION,
            format!(
                "Duplicate directive definition `@{}`",
                definition.name.value
            ),
            definition.name.span,
        );
        return;
    }
    let index = composer.schema.directives.len();
    composer
        .schema
        .directive_index
        .insert(definition.name.value.clone(), index);
    composer
        .schema
        .directives
        .push(SemanticDirectiveDefinition::from_source(definition));
}

fn add_type(composer: &mut Composer, semantic: SemanticType, name_span: crate::Span) {
    if composer.schema.type_index.contains_key(&semantic.name) {
        composer.report(
            super::SCHEMA_DUPLICATE_TYPE,
            format!("Duplicate type definition `{}`", semantic.name),
            name_span,
        );
        return;
    }
    let index = composer.schema.types.len();
    composer
        .schema
        .type_index
        .insert(semantic.name.clone(), index);
    composer.schema.types.push(semantic);
}

fn base_type(
    name: &str,
    name_span: crate::Span,
    kind: TypeKind,
    description: Option<crate::language::ast::Description>,
    directives: Vec<crate::language::ast::Directive>,
    span: crate::Span,
) -> SemanticType {
    let directive_origins = vec![SchemaOrigin::Definition(span); directives.len()];
    SemanticType {
        name: name.to_owned(),
        name_span: Some(name_span),
        kind,
        description,
        directives,
        directive_origins,
        origins: vec![SchemaOrigin::Definition(span)],
        implements_interfaces: Vec::new(),
        implemented_interface_index: Default::default(),
        fields: Vec::new(),
        union_members: Vec::new(),
        union_member_index: Default::default(),
        enum_values: Vec::new(),
        input_fields: Vec::new(),
        field_index: Default::default(),
        field_argument_index: Default::default(),
        enum_value_index: Default::default(),
        input_field_index: Default::default(),
    }
}
