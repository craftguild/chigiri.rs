use super::{
    SemanticTypeRefKind, compose_schema, parse_schema_coordinate, resolve_schema_coordinate,
};
use crate::language::ast::{Directive, DirectiveLocation, Value};
use crate::language::parse_type_system_document;
use crate::{ExecutionLimits, SourceId};

fn applied<'a>(directives: &'a [Directive], name: &str) -> &'a Directive {
    directives
        .iter()
        .find(|directive| directive.name.value == name)
        .unwrap()
}

fn string_argument<'a>(directive: &'a Directive, name: &str) -> &'a str {
    let argument = directive
        .arguments
        .iter()
        .find(|argument| argument.name.value == name)
        .unwrap();
    match &argument.value.value {
        Value::String(value) => &value.value,
        _ => panic!("The argument needs to be String."),
    }
}

#[test]
fn exposes_rich_sdl_metadata_without_loss() {
    let source = r#"
      "Schema docs"
      schema { query: Query }

      "Audit docs"
      directive @audit(
        "Level docs"
        level: Int = 1
      ) repeatable on OBJECT | FIELD_DEFINITION

      "URL docs"
      scalar URL @specifiedBy(url: "https://example.com/url")

      interface Node { id: ID! }
      interface Timestamped { createdAt: String! }

      "User docs"
      type User implements Node & Timestamped @audit(level: 2) {
        id: ID!
        createdAt: String!
        homepage: URL
        "Legacy field docs"
        old: String @deprecated(reason: "Use current")
        current: String @audit
      }

      union Search = User | Query
      enum Color { RED BLUE GREEN @deprecated(reason: "Use BLUE") }

      input Choice @oneOf { byId: ID, byName: String }
      input Filter {
        "Legacy input docs"
        legacy: String = "fallback" @deprecated(reason: "Use current")
        current: String
      }

      type Query {
        "Search docs"
        search("Filter docs" filter: Filter): [Search!]!
        pick(choice: Choice!): User
      }
    "#;
    let parsed =
        parse_type_system_document(SourceId::new(160), source, &ExecutionLimits::unbounded());
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    let output = compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    let schema = output.schema.unwrap();

    assert_eq!(schema.description().unwrap().value.value, "Schema docs");
    assert!(schema.type_by_name("__Schema").is_none());
    let coordinate = parse_schema_coordinate(SourceId::new(161), "__Schema").unwrap();
    assert!(
        resolve_schema_coordinate(&schema, &coordinate)
            .unwrap()
            .is_none()
    );
    assert!(
        schema
            .types()
            .iter()
            .all(|named_type| !named_type.name().starts_with("__"))
    );

    let url = schema.type_by_name("URL").unwrap();
    assert_eq!(url.description().unwrap().value.value, "URL docs");
    assert_eq!(
        string_argument(applied(url.directives(), "specifiedBy"), "url"),
        "https://example.com/url"
    );

    let choice = schema.type_by_name("Choice").unwrap();
    assert!(
        choice
            .directives()
            .iter()
            .any(|directive| directive.name.value == "oneOf")
    );

    let user = schema.type_by_name("User").unwrap();
    assert_eq!(user.description().unwrap().value.value, "User docs");
    assert_eq!(
        user.implemented_interfaces()
            .iter()
            .map(|interface| interface.value.as_str())
            .collect::<Vec<_>>(),
        ["Node", "Timestamped"]
    );
    assert!(matches!(
        applied(user.directives(), "audit").arguments[0].value.value,
        Value::Int(ref value) if value == "2"
    ));
    let old = user.field("old").unwrap();
    assert_eq!(
        old.description.as_ref().unwrap().value.value,
        "Legacy field docs"
    );
    assert_eq!(
        string_argument(applied(&old.directives, "deprecated"), "reason"),
        "Use current"
    );

    let filter = schema.type_by_name("Filter").unwrap();
    let legacy = &filter.input_fields()[0];
    assert_eq!(
        legacy.description.as_ref().unwrap().value.value,
        "Legacy input docs"
    );
    assert!(matches!(
        legacy.default_value.as_ref().unwrap().value,
        Value::String(ref value) if value.value == "fallback"
    ));
    assert_eq!(
        string_argument(applied(&legacy.directives, "deprecated"), "reason"),
        "Use current"
    );

    let audit = schema.directive_definition("audit").unwrap();
    assert_eq!(audit.description().unwrap().value.value, "Audit docs");
    assert!(audit.is_repeatable());
    assert_eq!(audit.arguments()[0].name, "level");
    assert_eq!(
        audit.arguments()[0]
            .source_definition
            .as_ref()
            .unwrap()
            .description
            .as_ref()
            .unwrap()
            .value
            .value,
        "Level docs"
    );
    assert!(matches!(
        audit.arguments()[0].default_value,
        Some(Value::Int(ref value)) if value == "1"
    ));
    assert_eq!(
        audit
            .locations()
            .iter()
            .map(|location| location.value)
            .collect::<Vec<_>>(),
        [
            DirectiveLocation::Object,
            DirectiveLocation::FieldDefinition,
        ]
    );

    let deprecated = schema.directive_definition("deprecated").unwrap();
    assert!(matches!(
        deprecated.arguments()[0].ty.kind,
        SemanticTypeRefKind::NonNull(_)
    ));
    assert!(matches!(
        deprecated.arguments()[0].default_value,
        Some(Value::String(ref value)) if value.value == "No longer supported"
    ));

    assert_eq!(
        schema
            .type_by_name("Search")
            .unwrap()
            .union_members()
            .iter()
            .map(|member| member.value.as_str())
            .collect::<Vec<_>>(),
        ["User", "Query"]
    );
    assert_eq!(
        schema
            .type_by_name("Color")
            .unwrap()
            .enum_values()
            .iter()
            .map(|value| value.name.value.as_str())
            .collect::<Vec<_>>(),
        ["RED", "BLUE", "GREEN"]
    );
    let search = schema
        .type_by_name("Query")
        .unwrap()
        .field("search")
        .unwrap();
    assert_eq!(
        search.description.as_ref().unwrap().value.value,
        "Search docs"
    );
    assert_eq!(
        search.arguments[0]
            .description
            .as_ref()
            .unwrap()
            .value
            .value,
        "Filter docs"
    );
}
