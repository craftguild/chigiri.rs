use super::tests_support::assert_valid;
use crate::language::ast::TypeSystemDefinition;

#[test]
fn preserves_descriptions_for_every_definition_kind() {
    let definitions = assert_valid(concat!(
        "\"schema\" schema { query: Query }\n",
        "\"scalar\" scalar Date\n",
        "\"object\" type Query { value: String }\n",
        "\"interface\" interface Node { id: ID }\n",
        "\"union\" union Search = Query\n",
        "\"enum\" enum Direction { NORTH }\n",
        "\"input\" input Filter { term: String }\n",
        "\"directive\" directive @tag on FIELD_DEFINITION\n",
    ));
    let descriptions = definitions
        .iter()
        .map(|definition| match definition {
            TypeSystemDefinition::Schema(value) => value.description.as_ref(),
            TypeSystemDefinition::Scalar(value) => value.description.as_ref(),
            TypeSystemDefinition::Object(value) => value.description.as_ref(),
            TypeSystemDefinition::Interface(value) => value.description.as_ref(),
            TypeSystemDefinition::Union(value) => value.description.as_ref(),
            TypeSystemDefinition::Enum(value) => value.description.as_ref(),
            TypeSystemDefinition::InputObject(value) => value.description.as_ref(),
            TypeSystemDefinition::Directive(value) => value.description.as_ref(),
            _ => panic!("expected a definition."),
        })
        .map(|description| description.expect("description").value.value.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        descriptions,
        [
            "schema",
            "scalar",
            "object",
            "interface",
            "union",
            "enum",
            "input",
            "directive",
        ]
    );
}

#[test]
fn preserves_full_interface_and_member_description_syntax() {
    let source = "\"I\" interface I implements & N @d { \"f\" f(\"a\" a: Int = 1 @x): String @y }";
    let definitions = assert_valid(source);
    let TypeSystemDefinition::Interface(interface) = &definitions[0] else {
        panic!("expected an interface definition.");
    };
    let field = &interface.fields[0];
    let argument = &field.arguments[0];

    assert_eq!(interface.description.as_ref().unwrap().value.value, "I");
    assert_eq!(interface.implements_interfaces[0].value, "N");
    assert_eq!(interface.directives[0].name.value, "d");
    assert_eq!(field.description.as_ref().unwrap().value.value, "f");
    assert_eq!(argument.description.as_ref().unwrap().value.value, "a");
    assert_eq!(
        argument.default_value.as_ref().unwrap().span.start,
        source.find("1 @x").unwrap()
    );
    assert_eq!(argument.directives[0].name.value, "x");
    assert_eq!(field.directives[0].name.value, "y");
}
