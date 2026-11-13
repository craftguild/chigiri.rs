//! Injects scalars and directives specified by GraphQL September 2025.

use crate::language::ast::DirectiveLocation;
use std::collections::BTreeMap;

use super::{
    SchemaOrigin, SemanticDirectiveArgument, SemanticDirectiveDefinition,
    SemanticDirectiveLocation, SemanticSchema, SemanticType, SemanticTypeRef, TypeKind,
};

const BUILT_IN_SCALARS: [&str; 5] = ["Int", "Float", "String", "Boolean", "ID"];

pub(super) fn empty_schema_with_builtins() -> SemanticSchema {
    let mut schema = SemanticSchema {
        types: Vec::new(),
        type_index: BTreeMap::new(),
        directives: Vec::new(),
        directive_index: BTreeMap::new(),
        roots: Default::default(),
        description: None,
        schema_directives: Vec::new(),
        schema_origins: Vec::new(),
        possible_object_index: BTreeMap::new(),
    };
    for name in BUILT_IN_SCALARS {
        let index = schema.types.len();
        schema.type_index.insert(name.to_owned(), index);
        schema.types.push(SemanticType {
            name: name.to_owned(),
            name_span: None,
            kind: TypeKind::Scalar,
            description: None,
            directives: Vec::new(),
            directive_origins: Vec::new(),
            origins: vec![SchemaOrigin::BuiltIn],
            implements_interfaces: Vec::new(),
            implemented_interface_index: Default::default(),
            fields: Vec::new(),
            union_members: Vec::new(),
            union_member_index: Default::default(),
            enum_values: Vec::new(),
            input_fields: Vec::new(),
            field_index: BTreeMap::new(),
            field_argument_index: BTreeMap::new(),
            enum_value_index: BTreeMap::new(),
            input_field_index: BTreeMap::new(),
        });
    }
    for directive in built_in_directives() {
        let index = schema.directives.len();
        schema.directive_index.insert(directive.name.clone(), index);
        schema.directives.push(directive);
    }
    schema
}

fn built_in_directives() -> Vec<SemanticDirectiveDefinition> {
    vec![
        boolean_condition("skip"),
        boolean_condition("include"),
        deprecated(),
        specified_by(),
        one_of(),
    ]
}

fn boolean_condition(name: &str) -> SemanticDirectiveDefinition {
    directive(
        name,
        vec![argument(
            "if",
            SemanticTypeRef::non_null(SemanticTypeRef::named("Boolean")),
            None,
        )],
        &[
            DirectiveLocation::Field,
            DirectiveLocation::FragmentSpread,
            DirectiveLocation::InlineFragment,
        ],
    )
}

fn deprecated() -> SemanticDirectiveDefinition {
    directive(
        "deprecated",
        vec![argument(
            "reason",
            SemanticTypeRef::non_null(SemanticTypeRef::named("String")),
            Some(super::directive_model::string_default(
                "No longer supported",
            )),
        )],
        &[
            DirectiveLocation::FieldDefinition,
            DirectiveLocation::ArgumentDefinition,
            DirectiveLocation::InputFieldDefinition,
            DirectiveLocation::EnumValue,
        ],
    )
}

fn specified_by() -> SemanticDirectiveDefinition {
    directive(
        "specifiedBy",
        vec![argument(
            "url",
            SemanticTypeRef::non_null(SemanticTypeRef::named("String")),
            None,
        )],
        &[DirectiveLocation::Scalar],
    )
}

fn one_of() -> SemanticDirectiveDefinition {
    directive("oneOf", Vec::new(), &[DirectiveLocation::InputObject])
}

fn argument(
    name: &str,
    ty: SemanticTypeRef,
    default_value: Option<crate::language::ast::Value>,
) -> SemanticDirectiveArgument {
    SemanticDirectiveArgument {
        name: name.to_owned(),
        ty,
        default_value,
        origin: SchemaOrigin::BuiltIn,
        source_definition: None,
    }
}

fn directive(
    name: &str,
    arguments: Vec<SemanticDirectiveArgument>,
    locations: &[DirectiveLocation],
) -> SemanticDirectiveDefinition {
    let argument_index = arguments
        .iter()
        .enumerate()
        .map(|(index, argument)| (argument.name.clone(), index))
        .collect();
    let required_argument_indices = super::directive_model::required_argument_indices(&arguments);
    let location_mask = locations.iter().copied().fold(0, |mask, location| {
        mask | super::directive_model::location_bit(location)
    });
    SemanticDirectiveDefinition {
        name: name.to_owned(),
        description: None,
        arguments,
        argument_index,
        required_argument_indices,
        repeatable: false,
        locations: locations
            .iter()
            .copied()
            .map(|value| SemanticDirectiveLocation { value, span: None })
            .collect(),
        location_mask,
        origin: SchemaOrigin::BuiltIn,
        source_definition: None,
    }
}
