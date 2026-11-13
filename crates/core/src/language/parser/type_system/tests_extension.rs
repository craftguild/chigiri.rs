use super::tests_support::assert_valid;
use crate::language::ast::{OperationType, TypeSystemDefinition, Value};

#[test]
fn schema_scalar_and_union_extensions_preserve_allowed_elements() {
    let source = concat!(
        "extend schema @tag\n",
        "extend schema @audit { mutation: Mutation subscription: Events }\n",
        "extend scalar Date @specifiedBy(url: \"https://example.test/date\") @tag\n",
        "extend union Search @tag\n",
        "extend union Search @audit = | Photo | Person | Photo\n",
    );
    let definitions = assert_valid(source);

    let TypeSystemDefinition::SchemaExtension(directive_only) = &definitions[0] else {
        panic!("expected a schema extension.");
    };
    assert_eq!(directive_only.directives[0].name.value, "tag");
    assert!(directive_only.root_operations.is_empty());

    let TypeSystemDefinition::SchemaExtension(with_roots) = &definitions[1] else {
        panic!("expected a schema extension.");
    };
    assert_eq!(
        with_roots.root_operations[0].operation_type.value,
        OperationType::Mutation
    );
    assert_eq!(
        with_roots.root_operations[1].operation_type.value,
        OperationType::Subscription
    );

    let TypeSystemDefinition::ScalarExtension(scalar) = &definitions[2] else {
        panic!("expected a scalar extension.");
    };
    assert_eq!(scalar.directives.len(), 2);
    assert!(matches!(
        &scalar.directives[0].arguments[0].value.value,
        Value::String(value) if value.value == "https://example.test/date"
    ));

    let TypeSystemDefinition::UnionExtension(directive_union) = &definitions[3] else {
        panic!("expected a union extension.");
    };
    assert!(directive_union.members.is_empty());
    let TypeSystemDefinition::UnionExtension(member_union) = &definitions[4] else {
        panic!("expected a union extension.");
    };
    let members = member_union
        .members
        .iter()
        .map(|member| member.value.as_str())
        .collect::<Vec<_>>();
    assert_eq!(members, ["Photo", "Person", "Photo"]);
}

#[test]
fn object_and_interface_extensions_parse_implements_directives_and_fields() {
    let source = concat!(
        "extend type Product implements & Node & Resource\n",
        "extend type Product @tag @tag\n",
        "extend type Product implements Node @audit { sku: ID sku: ID }\n",
        "extend interface Resource implements Node\n",
        "extend interface Resource @tag\n",
        "extend interface Resource implements Node @audit { url: String }\n",
    );
    let definitions = assert_valid(source);

    let TypeSystemDefinition::ObjectExtension(interfaces) = &definitions[0] else {
        panic!("expected an object extension.");
    };
    assert_eq!(interfaces.implements_interfaces[0].value, "Node");
    assert_eq!(interfaces.implements_interfaces[1].value, "Resource");
    assert!(interfaces.fields.is_empty());

    let TypeSystemDefinition::ObjectExtension(directives) = &definitions[1] else {
        panic!("expected an object extension.");
    };
    assert_eq!(directives.directives.len(), 2);
    assert_ne!(directives.directives[0].span, directives.directives[1].span);

    let TypeSystemDefinition::ObjectExtension(fields) = &definitions[2] else {
        panic!("expected an object extension.");
    };
    assert_eq!(fields.fields.len(), 2);
    assert_eq!(fields.fields[0].name.value, fields.fields[1].name.value);
    assert_eq!(fields.directives[0].name.value, "audit");

    assert!(matches!(
        &definitions[3],
        TypeSystemDefinition::InterfaceExtension(value)
            if value.implements_interfaces[0].value == "Node"
    ));
    assert!(matches!(
        &definitions[4],
        TypeSystemDefinition::InterfaceExtension(value) if value.directives.len() == 1
    ));
    assert!(matches!(
        &definitions[5],
        TypeSystemDefinition::InterfaceExtension(value) if value.fields[0].name.value == "url"
    ));
}

#[test]
fn enum_and_input_extension_recognize_directive_only_or_member_block() {
    let source = concat!(
        "extend enum Direction @tag\n",
        "extend enum Direction @audit { EAST WEST EAST }\n",
        "extend input Filter @tag\n",
        "extend input Filter @audit { id: ID id: ID term: String = \"all\" }\n",
    );
    let definitions = assert_valid(source);

    assert!(matches!(
        &definitions[0],
        TypeSystemDefinition::EnumExtension(value)
            if value.values.is_empty() && value.directives.len() == 1
    ));
    let TypeSystemDefinition::EnumExtension(enum_members) = &definitions[1] else {
        panic!("expected enum extension.");
    };
    assert_eq!(enum_members.values.len(), 3);
    assert_eq!(
        enum_members.values[0].name.value,
        enum_members.values[2].name.value
    );

    assert!(matches!(
        &definitions[2],
        TypeSystemDefinition::InputObjectExtension(value)
            if value.fields.is_empty() && value.directives.len() == 1
    ));
    let TypeSystemDefinition::InputObjectExtension(input_members) = &definitions[3] else {
        panic!("expected an input extension.");
    };
    assert_eq!(input_members.fields.len(), 3);
    assert_eq!(
        input_members.fields[0].name.value,
        input_members.fields[1].name.value
    );
    assert!(matches!(
        input_members.fields[2].default_value.as_ref().map(|value| &value.value),
        Some(Value::String(value)) if value.value == "all"
    ));
}
