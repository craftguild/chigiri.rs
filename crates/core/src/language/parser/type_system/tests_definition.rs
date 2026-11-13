use super::tests_support::assert_valid;
use crate::language::ast::{
    DirectiveLocation, OperationType, StringStyle, TypeRefKind, TypeSystemDefinition, Value,
};
use crate::{SourceId, Span};

#[test]
fn schema_definition_has_a_description_root_sequence_duplication_and_span() {
    let source = concat!(
        "\"schema description\"\n",
        "schema @audit(level: 1) {\n",
        "  query: Root\n  mutation: Mutation\n  query: OtherRoot\n",
        "  subscription: Events\n}\n",
    );
    let definitions = assert_valid(source);
    let TypeSystemDefinition::Schema(schema) = &definitions[0] else {
        panic!("expected a schema definition.");
    };

    assert_eq!(
        schema.span,
        Span::new(SourceId::new(42), 0, source.trim_end().len())
    );
    let description = schema
        .description
        .as_ref()
        .expect("expected a description.");
    assert_eq!(description.value.value, "schema description");
    assert_eq!(description.value.style, StringStyle::Quoted);
    assert_eq!(schema.directives[0].name.value, "audit");
    assert_eq!(schema.directives[0].arguments[0].name.value, "level");
    assert_eq!(schema.root_operations.len(), 4);
    assert_eq!(
        schema.root_operations[0].operation_type.value,
        OperationType::Query
    );
    assert_eq!(
        schema.root_operations[2].operation_type.value,
        OperationType::Query
    );
    assert_eq!(
        schema.root_operations[3].operation_type.value,
        OperationType::Subscription
    );
    assert_eq!(schema.root_operations[0].named_type.value, "Root");
    assert_eq!(schema.root_operations[2].named_type.value, "OtherRoot");
}

#[test]
fn object_definitions_parse_interfaces_fields_arguments_defaults_and_directives() {
    let source = concat!(
        "\"\"\"Product docs\"\"\"\n",
        "type Product implements & Node & Node & Resource @tag {\n",
        "  \"\"\"A field\"\"\"\n",
        "  item(\n",
        "    \"first\" id: ID!\n",
        "    id: ID! = \"fallback\" @deprecated\n",
        "  ): [String!]! @tag @tag\n",
        "}\n",
    );
    let definitions = assert_valid(source);
    let TypeSystemDefinition::Object(object) = &definitions[0] else {
        panic!("expected an object definition.");
    };

    assert_eq!(
        object.description.as_ref().unwrap().value.style,
        StringStyle::Block
    );
    assert_eq!(object.implements_interfaces[0].value, "Node");
    assert_eq!(object.implements_interfaces[1].value, "Node");
    assert_eq!(object.implements_interfaces[2].value, "Resource");
    let field = &object.fields[0];
    assert_eq!(field.description.as_ref().unwrap().value.value, "A field");
    assert_eq!(field.arguments.len(), 2);
    assert_eq!(field.arguments[0].name.value, "id");
    assert_eq!(field.arguments[1].name.value, "id");
    let default = field.arguments[1].default_value.as_ref().unwrap();
    assert!(matches!(&default.value, Value::String(value) if value.value == "fallback"));
    assert!(matches!(field.ty.kind, TypeRefKind::NonNull(_)));
    assert_eq!(field.directives[0].name.value, "tag");
    assert_eq!(field.directives[1].name.value, "tag");
    assert_ne!(field.directives[0].span, field.directives[1].span);
    assert_eq!(object.span.end, source.trim_end().len());
}

#[test]
fn named_types_and_directive_definitions_preserve_all_syntax_elements() {
    let source = concat!(
        "\"date\" scalar Date @specifiedBy(url: \"https://example.test/date\")\n",
        "union Search @tag = | Photo | Person | Photo\n",
        "enum Direction @tag { \"north\" NORTH NORTH @deprecated }\n",
        "input Filter @oneOf { id: ID \"term\" term: String = \"all\" @deprecated }\n",
        "\"dir\" directive @auth(\"role\" role: Role! = ADMIN @deprecated) ",
        "repeatable on | FIELD | FIELD_DEFINITION | INPUT_FIELD_DEFINITION\n",
    );
    let definitions = assert_valid(source);

    let TypeSystemDefinition::Scalar(scalar) = &definitions[0] else {
        panic!("expected a scalar definition.");
    };
    assert_eq!(scalar.description.as_ref().unwrap().value.value, "date");
    assert_eq!(scalar.directives[0].arguments[0].name.value, "url");

    let TypeSystemDefinition::Union(union) = &definitions[1] else {
        panic!("expected a union definition.");
    };
    let members = union
        .members
        .iter()
        .map(|name| name.value.as_str())
        .collect::<Vec<_>>();
    assert_eq!(members, ["Photo", "Person", "Photo"]);

    let TypeSystemDefinition::Enum(enum_type) = &definitions[2] else {
        panic!("expected enum definition.");
    };
    assert_eq!(
        enum_type.values[0]
            .description
            .as_ref()
            .unwrap()
            .value
            .value,
        "north"
    );
    assert_eq!(
        enum_type.values[0].name.value,
        enum_type.values[1].name.value
    );

    let TypeSystemDefinition::InputObject(input) = &definitions[3] else {
        panic!("expected an input object definition.");
    };
    assert_eq!(input.directives[0].name.value, "oneOf");
    assert_eq!(
        input.fields[1].description.as_ref().unwrap().value.value,
        "term"
    );

    let TypeSystemDefinition::Directive(directive) = &definitions[4] else {
        panic!("expected a directive definition.");
    };
    assert!(directive.is_repeatable());
    assert_eq!(directive.arguments[0].name.value, "role");
    let locations = directive
        .locations
        .iter()
        .map(|item| item.value)
        .collect::<Vec<_>>();
    assert_eq!(
        locations,
        [
            DirectiveLocation::Field,
            DirectiveLocation::FieldDefinition,
            DirectiveLocation::InputFieldDefinition,
        ]
    );
}

#[test]
fn september_2025_accepts_member_blocks() {
    let source = concat!(
        "type Empty\ninterface EmptyInterface\nenum EmptyEnum\n",
        "input EmptyInput\nunion EmptyUnion\nscalar EmptyScalar\n",
    );
    let definitions = assert_valid(source);

    assert_eq!(definitions.len(), 6);
    assert!(
        matches!(&definitions[0], TypeSystemDefinition::Object(value) if value.fields.is_empty())
    );
    assert!(
        matches!(&definitions[1], TypeSystemDefinition::Interface(value) if value.fields.is_empty())
    );
    assert!(
        matches!(&definitions[2], TypeSystemDefinition::Enum(value) if value.values.is_empty())
    );
    assert!(
        matches!(&definitions[3], TypeSystemDefinition::InputObject(value) if value.fields.is_empty())
    );
    assert!(
        matches!(&definitions[4], TypeSystemDefinition::Union(value) if value.members.is_empty())
    );
}

#[test]
fn directive_definitions_preserve_all_locations_in_source_order() {
    let source = concat!(
        "directive @all on QUERY | QUERY | MUTATION | SUBSCRIPTION | FIELD | ",
        "FRAGMENT_DEFINITION | FRAGMENT_SPREAD | INLINE_FRAGMENT | VARIABLE_DEFINITION | ",
        "SCHEMA | SCALAR | OBJECT | FIELD_DEFINITION | ARGUMENT_DEFINITION | INTERFACE | ",
        "UNION | ENUM | ENUM_VALUE | INPUT_OBJECT | INPUT_FIELD_DEFINITION",
    );
    let definitions = assert_valid(source);
    let TypeSystemDefinition::Directive(directive) = &definitions[0] else {
        panic!("expected a directive definition.");
    };
    let actual = directive
        .locations
        .iter()
        .map(|item| item.value)
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            DirectiveLocation::Query,
            DirectiveLocation::Query,
            DirectiveLocation::Mutation,
            DirectiveLocation::Subscription,
            DirectiveLocation::Field,
            DirectiveLocation::FragmentDefinition,
            DirectiveLocation::FragmentSpread,
            DirectiveLocation::InlineFragment,
            DirectiveLocation::VariableDefinition,
            DirectiveLocation::Schema,
            DirectiveLocation::Scalar,
            DirectiveLocation::Object,
            DirectiveLocation::FieldDefinition,
            DirectiveLocation::ArgumentDefinition,
            DirectiveLocation::Interface,
            DirectiveLocation::Union,
            DirectiveLocation::Enum,
            DirectiveLocation::EnumValue,
            DirectiveLocation::InputObject,
            DirectiveLocation::InputFieldDefinition,
        ]
    );
}
