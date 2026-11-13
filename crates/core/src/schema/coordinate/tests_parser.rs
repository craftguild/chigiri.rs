use super::{
    SCHEMA_COORDINATE_SYNTAX, SCHEMA_COORDINATE_TRAILING_INPUT, SchemaCoordinate,
    parse_schema_coordinate,
};
use crate::SourceId;

fn parse(source: &str) -> SchemaCoordinate {
    parse_schema_coordinate(SourceId::new(7), source).expect("coordinate should parse")
}

#[test]
fn distinguishes_and_canonicalizes_five_coordinate_forms() {
    let cases = [
        ("Query", "type"),
        ("Query.user", "member"),
        ("Query.user(id:)", "field argument"),
        ("@skip", "directive"),
        ("@skip(if:)", "directive argument"),
    ];

    for (source, expected_kind) in cases {
        let coordinate = parse(source);
        let actual_kind = match coordinate {
            SchemaCoordinate::Type { .. } => "type",
            SchemaCoordinate::Member { .. } => "member",
            SchemaCoordinate::FieldArgument { .. } => "field argument",
            SchemaCoordinate::Directive { .. } => "directive",
            SchemaCoordinate::DirectiveArgument { .. } => "directive argument",
        };
        assert_eq!(actual_kind, expected_kind);
        assert_eq!(coordinate.to_string(), source);
    }
}

#[test]
fn preserves_the_name_byte_span() {
    let SchemaCoordinate::FieldArgument {
        type_name,
        field_name,
        argument_name,
    } = parse("Query.user(id:)")
    else {
        panic!("expected field argument")
    };

    assert_eq!((type_name.span.start, type_name.span.end), (0, 5));
    assert_eq!((field_name.span.start, field_name.span.end), (6, 10));
    assert_eq!((argument_name.span.start, argument_name.span.end), (11, 13));
}

#[test]
fn rejects_empty_unicode_and_ignored_tokens() {
    let invalid = [
        "",
        "Type of the thing.",
        "Týpe",
        " Query",
        "Query ",
        "Query\n",
        "Query,member",
        "Query#comment",
        "@skip (if:)",
        "\u{feff}Query",
    ];

    for source in invalid {
        assert!(
            parse_schema_coordinate(SourceId::new(0), source).is_err(),
            "accepted {source:?}"
        );
    }
}

#[test]
fn invalid_coordinates_return_only_syntax_errors() {
    let invalid = [
        "@",
        "Query.",
        "Query..user",
        "Query.user()",
        "Query.user(id)",
        "Query.user(:)",
        "@skip()",
        "@skip(if)",
    ];

    for source in invalid {
        let error = parse_schema_coordinate(SourceId::new(0), source).unwrap_err();
        assert_eq!(error.code, SCHEMA_COORDINATE_SYNTAX, "source: {source}");
    }
}

#[test]
fn rejects_trailing_characters_after_a_valid_prefix_with_a_stable_code() {
    let invalid = [
        "Query.user.extra",
        "Query.user(id:)extra",
        "@skip.extra",
        "@skip(if:).extra",
    ];

    for source in invalid {
        let error = parse_schema_coordinate(SourceId::new(0), source).unwrap_err();
        assert_eq!(
            error.code, SCHEMA_COORDINATE_TRAILING_INPUT,
            "source: {source}"
        );
    }
}
