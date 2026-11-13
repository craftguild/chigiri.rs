use super::{
    SCHEMA_DIRECTIVE_ARGUMENT_VARIABLE, SCHEMA_DUPLICATE_DIRECTIVE_APPLICATION,
    SCHEMA_DUPLICATE_DIRECTIVE_ARGUMENT, SCHEMA_DUPLICATE_MEMBER,
    SCHEMA_INVALID_DIRECTIVE_LOCATION, SCHEMA_MISSING_DIRECTIVE_ARGUMENT, SCHEMA_UNKNOWN_DIRECTIVE,
    SCHEMA_UNKNOWN_DIRECTIVE_ARGUMENT, compose_schema,
};
use crate::language::ast::{Definition, Name, ObjectField, TypeSystemDefinition, Value, ValueNode};
use crate::language::parse_type_system_document;
use crate::{ExecutionLimits, SourceId, Span};

fn parse(id: usize, source: &str) -> crate::language::ast::Document {
    let output =
        parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

#[test]
fn accepts_all_type_system_locations_and_builtin_directives() {
    let document = parse(
        120,
        r#"
        directive @argTag on ARGUMENT_DEFINITION
        directive @tag(label: String @argTag) repeatable on
          SCHEMA | SCALAR | OBJECT | FIELD_DEFINITION | ARGUMENT_DEFINITION |
          INTERFACE | UNION | ENUM | ENUM_VALUE | INPUT_OBJECT | INPUT_FIELD_DEFINITION
        schema @tag { query: Query }
        scalar URL @tag @specifiedBy(url: "https://example.com")
        interface Node @tag { id(format: String @tag): ID @tag }
        type Query implements Node @tag {
          id(format: String @tag): ID @tag
          choose(input: Choice @deprecated): Status
        }
        union Search @tag = Query
        enum Status @tag { OLD @tag @deprecated }
        input Choice @tag @oneOf { text: String @tag }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn repeatable_directives_preserve_source_order_across_definitions_and_extensions() {
    let extension = parse(121, "extend type Query @tag(value: \"extension\")");
    let definition = parse(
        122,
        concat!(
            "directive @tag(value: String) repeatable on OBJECT ",
            "type Query @tag(value: \"definition\") { ok: String }",
        ),
    );
    let output = compose_schema(&[extension, definition], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    let schema = output.schema.unwrap();
    let values = schema
        .type_by_name("Query")
        .unwrap()
        .directives()
        .iter()
        .map(|directive| match &directive.arguments[0].value.value {
            Value::String(value) => value.value.as_str(),
            _ => panic!("The tag value needs to be String."),
        })
        .collect::<Vec<_>>();
    assert_eq!(values, ["extension", "definition"]);
}

#[test]
fn rejects_undefined_invalid_location_and_repeated_nonrepeatable_directives() {
    let document = parse(
        123,
        r#"
        directive @once on OBJECT
        directive @scalarOnly on SCALAR
        scalar Token
        type Query @missing @once { ok: String }
        extend type Query @once @scalarOnly
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let codes = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();

    assert!(output.schema.is_none());
    assert!(codes.contains(&SCHEMA_UNKNOWN_DIRECTIVE));
    assert!(codes.contains(&SCHEMA_INVALID_DIRECTIVE_LOCATION));
    assert!(codes.contains(&SCHEMA_DUPLICATE_DIRECTIVE_APPLICATION));
}

#[test]
fn distinguishes_provided_unknown_and_missing_required_directive_arguments() {
    let document = parse(
        124,
        concat!(
            "directive @policy(required: String!, defaulted: Int! = 1) ",
            "repeatable on OBJECT ",
            "type Query ",
            "@policy(required: \"x\", required: \"y\", extra: true) ",
            "@policy { ok: String }",
        ),
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_DUPLICATE_DIRECTIVE_ARGUMENT)
            .count(),
        1
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_UNKNOWN_DIRECTIVE_ARGUMENT)
            .count(),
        1
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_MISSING_DIRECTIVE_ARGUMENT)
            .count(),
        1
    );
}

#[test]
fn rejects_nested_ast_variables_in_const_positions() {
    let mut document = parse(
        125,
        concat!(
            "directive @tag(value: String) on OBJECT ",
            "type Query @tag(value: \"ok\") { ok: String }",
        ),
    );
    let variable_span = Span::new(SourceId::new(125), 8, 10);
    let variable = ValueNode::new(
        Value::Variable(Name::new(
            "x".to_owned(),
            Span::new(SourceId::new(125), 9, 10),
        )),
        variable_span,
    );
    let nested = ValueNode::new(
        Value::Object(vec![ObjectField {
            name: Name::new("nested".to_owned(), Span::new(SourceId::new(125), 0, 6)),
            value: variable,
            span: Span::new(SourceId::new(125), 0, 10),
        }]),
        Span::new(SourceId::new(125), 0, 11),
    );
    let value = ValueNode::new(
        Value::List(vec![nested]),
        Span::new(SourceId::new(125), 0, 12),
    );
    let argument = document
        .definitions
        .iter_mut()
        .find_map(|definition| match definition {
            Definition::TypeSystem(TypeSystemDefinition::Object(object))
                if object.name.value == "Query" =>
            {
                Some(&mut object.directives[0].arguments[0])
            }
            _ => None,
        })
        .unwrap();
    argument.value = value;

    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_DIRECTIVE_ARGUMENT_VARIABLE)
        .unwrap();

    assert!(output.schema.is_none());
    assert_eq!(diagnostic.span, Some(variable_span));
}

#[test]
fn directive_validation_preserves_structural_limit_and_source_diagnostics() {
    let source = "type Query @first @second @third { ok: String }";
    let document = parse(126, source);
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 2;
    let output = compose_schema(&[document], &limits);
    let spans = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.span.unwrap().start)
        .collect::<Vec<_>>();

    assert!(output.schema.is_none());
    assert_eq!(output.diagnostics.len(), 2);
    assert!(output.diagnostics_were_truncated());
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == SCHEMA_UNKNOWN_DIRECTIVE)
    );
    assert_eq!(
        spans,
        [
            source.find("first").unwrap(),
            source.find("second").unwrap(),
        ]
    );
}

#[test]
fn indexes_many_directive_arguments() {
    const ARGUMENTS: usize = 10_000;
    let mut source = String::with_capacity(ARGUMENTS * 28);
    source.push_str("directive @bulk(");
    for index in 0..ARGUMENTS {
        source.push_str(&format!("a{index}: Int "));
    }
    source.push_str(") on OBJECT\ntype Query @bulk(");
    for index in 0..ARGUMENTS {
        source.push_str(&format!("a{index}: {index} "));
    }
    source.push_str(") { ok: String }");
    let document = parse(127, &source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn validates_large_directive_definitions_and_applications_without_quadratic_work() {
    const ARGUMENTS: usize = 10_000;
    const APPLICATIONS: usize = 10_000;
    let mut source = String::with_capacity((ARGUMENTS + APPLICATIONS) * 14);
    source.push_str("directive @bulk(");
    for index in 0..ARGUMENTS {
        source.push_str(&format!("a{index}: Int "));
    }
    source.push_str(") repeatable on OBJECT\ntype Query ");
    for _ in 0..APPLICATIONS {
        source.push_str("@bulk ");
    }
    source.push_str("{ ok: String }");
    let document = parse(128, &source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn repeated_location_registration_is_bounded_before_rejection() {
    const LOCATIONS: usize = 10_000;
    const APPLICATIONS: usize = 10_000;
    let mut source = String::with_capacity((LOCATIONS + APPLICATIONS) * 10);
    source.push_str("directive @bulk repeatable on ");
    for _ in 0..LOCATIONS {
        source.push_str("SCALAR | ");
    }
    source.push_str("OBJECT\ntype Query ");
    for _ in 0..APPLICATIONS {
        source.push_str("@bulk ");
    }
    source.push_str("{ ok: String }");
    let document = parse(129, &source);
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 1;
    let output = compose_schema(&[document], &limits);

    assert!(output.schema.is_none());
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        SCHEMA_DUPLICATE_MEMBER
    );
    assert!(output.diagnostics_were_truncated());
}
