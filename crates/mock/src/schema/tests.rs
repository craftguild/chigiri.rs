//! Test multi-file SDL, vocabulary consistency, source locations, and pre-composition budgets.

use std::fmt::Write as _;

use super::{VOCABULARY, VOCABULARY_SOURCE, parse_sources};
use chigiri::core::Source;

fn parse(text: &str) -> Result<chigiri::experimental::OwnedSchema, super::SchemaError> {
    parse_sources(vec![Source::new("schema.graphql", text)])
}

#[test]
fn rsm_01_sources_compose_with_extensions_wrappers_and_vocab_digest() {
    let sources = vec![
        Source::new(
            "models.graphql",
            "type Item {id: ID!} input Choice @oneOf {id: ID label: String}",
        ),
        Source::new(
            "query.graphql",
            "extend type Item {label: String} type Query {items: [Item!]!}",
        ),
    ];
    let first = parse_sources(sources.clone()).unwrap();
    let second = parse_sources(sources).unwrap();
    assert_eq!(first.digest(), second.digest());
    assert_eq!(first.sources().len(), 3);
    assert_eq!(first.sources()[2].name(), VOCABULARY_SOURCE);
    let item = first.semantic().type_by_name("Item").unwrap();
    assert_eq!(item.fields().len(), 2);
    assert_eq!(item.fields()[1].span.source_id.index(), 1);
    assert!(
        first
            .semantic()
            .type_by_name("Choice")
            .unwrap()
            .directives()
            .iter()
            .any(|d| d.name.value == "oneOf")
    );
}

#[test]
fn rsm_02_equivalent_user_vocabulary_is_preserved_including_all_supplied() {
    let sdl = format!("{VOCABULARY}\ntype Query {{ x: Int @virtual }}");
    let schema = parse(&sdl).unwrap();
    assert_eq!(schema.sources()[0].text(), sdl);
    assert!(schema.sources()[1].text().starts_with('#'));
    assert!(!schema.sources()[1].text().contains("directive @"));
    let reordered = "directive @relation(name:String,references:[String!],fields:[String!]) on FIELD_DEFINITION type Query{x:Int}";
    assert!(parse(reordered).is_ok());
    // Directive names and GraphQL type names occupy separate namespaces; type spelling carries no directive meaning.
    assert!(parse("type chigiri_mock_model{x:Int} type Query{x:chigiri_mock_model}").is_ok());
}

#[test]
fn rsm_02_vocabulary_conflicts_unknown_words_locations_and_values_are_rejected() {
    for bad in [
        "directive @relation(fields:ID) on FIELD_DEFINITION",
        "directive @virtual repeatable on FIELD_DEFINITION",
        "directive @chigiri_mock_secret on FIELD_DEFINITION",
        "enum ChigiriMockRepresentation { STRING EXTRA }",
        "input ChigiriMockRepresentation { field: ID! }",
        "scalar ChigiriMockRepresentation",
    ] {
        let error = parse(&format!("{bad} type Query{{x:Int}}")).err().unwrap();
        assert!(error.code.contains("vocabulary"));
        assert!(error.span.is_some());
    }
    for bad in [
        "type Query @virtual {x:Int}",
        "type Query{x:Int @relation(unknown:\"X\")}",
        "type Query{x:Int @virtual @virtual}",
        "type Query{x:Int @constraint(regex:\"sensitive\")}",
    ] {
        let error = parse(bad).err().unwrap();
        assert_eq!(error.code, "invalid SDL");
        assert!(error.span.is_some());
        assert!(!error.to_string().contains("sensitive"));
    }
}

#[test]
fn rsm_16_sdl_bytes_sources_type_count_and_members_have_inclusive_limits() {
    let prefix = "type Query{x:Int} #";
    let exact = format!("{prefix}{}", "x".repeat(128 * 1024 - prefix.len()));
    assert!(parse(&exact).is_ok());
    assert_eq!(
        parse(&format!("{exact}x")).err().unwrap().code,
        "SDL byte budget exceeded"
    );
    let mut sources = vec![Source::new("query", "type Query{x:Int}")];
    for i in 0..63 {
        sources.push(Source::new(format!("s{i}"), format!("scalar S{i}")));
    }
    assert!(parse_sources(sources.clone()).is_ok());
    sources.push(Source::new("extra", "scalar Extra"));
    assert_eq!(
        parse_sources(sources).err().unwrap().code,
        "SDL source count exceeded"
    );
    let mut types = String::from("type Query{x:Int} ");
    for i in 0..255 {
        write!(types, "scalar S{i} ").unwrap();
    }
    assert!(parse(&types).is_ok());
    assert_eq!(
        parse(&format!("{types} scalar Extra")).err().unwrap().code,
        "schema shape budget exceeded"
    );
    let mut fields = String::new();
    for i in 0..4096 {
        write!(fields, "f{i}:Int ").unwrap();
    }
    assert!(parse(&format!("type Query{{{fields}}}")).is_ok());
    assert!(parse(&format!("{VOCABULARY} type Query{{{fields}}}")).is_ok());
    assert_eq!(
        parse(&format!("type Query{{{fields}extra:Int}}"))
            .err()
            .unwrap()
            .code,
        "schema shape budget exceeded"
    );
}

#[test]
fn rsm_02_source_names_are_unique_and_vocabulary_source_is_reserved() {
    let schema = parse("type Query{x:Int} type Subscription{tick:Int}").unwrap();
    assert_eq!(
        schema
            .semantic()
            .roots()
            .subscription()
            .unwrap()
            .type_name
            .as_str(),
        "Subscription"
    );
    for sources in [
        vec![
            Source::new("same", "type Query{x:Int}"),
            Source::new("same", "scalar Extra"),
        ],
        vec![Source::new(VOCABULARY_SOURCE, "type Query{x:Int}")],
    ] {
        assert_eq!(
            parse_sources(sources).err().unwrap().code,
            "duplicate or reserved source name"
        );
    }
}

#[test]
fn sd_01_old_directive_definitions_and_applications_are_explicit_errors() {
    for name in [
        "chigiri_mock_model",
        "chigiri_mock_relation",
        "chigiri_mock_inverse",
        "chigiri_mock_crud",
        "chigiri_mock_resolver",
        "chigiri_mock_value",
    ] {
        for sdl in [
            format!("type Query{{x:Int @{name}}}"),
            format!("directive @{name} on FIELD_DEFINITION type Query{{x:Int}}"),
            format!("input X{{x:Int @{name}}} type Query{{x(input:X):Int}}"),
            format!("type Query{{x(arg:Int @{name}):Int}}"),
        ] {
            let error = parse(&sdl).err().unwrap();
            assert!(error.code.starts_with("retired mock"), "{error}");
            assert!(error.span.is_some());
        }
    }
    assert!(parse(r#"type Query{x(arg:String = "@chigiri_mock_model"):Int}"#).is_ok());
}

#[test]
fn sd_12_constraint_vocabulary_has_exact_arguments_locations_and_no_extensions() {
    let schema =
        parse("type Query{x(arg:String @constraint(minLength:0)):String @constraint(maxLength:0)}")
            .unwrap();
    let definition = schema
        .semantic()
        .directive_definition("constraint")
        .unwrap();
    assert_eq!(definition.arguments().len(), 15);
    assert_eq!(definition.locations().len(), 3);
    for arg in ["errorMessage", "uniqueTypeName", "flags", "minBytes"] {
        assert!(parse(&format!("type Query{{x:String @constraint({arg}:\"x\")}}")).is_err());
    }
}

#[test]
fn rep_02_old_scalar_directive_is_rejected_at_ast_positions() {
    for sdl in [
        "directive @chigiri_mock_scalar(representation:ChigiriMockRepresentation!) on SCALAR type Query{x:Int}",
        "scalar Token @chigiri_mock_scalar(representation:STRING) type Query{x:Token}",
        "directive @chigiri_mock_scalar on SCALAR scalar Token @representation(type:STRING) type Query{x:Token}",
        "scalar Token @representation(type:STRING) @chigiri_mock_scalar(representation:STRING) type Query{x:Token}",
    ] {
        let error = parse(sdl).err().unwrap();
        assert_eq!(
            error.code,
            "retired mock scalar directive: use @representation(type: ...)"
        );
        assert!(error.span.is_some());
    }
    assert!(parse(
        r#""@chigiri_mock_scalar(representation:STRING) is ordinary text" scalar Token @representation(type:STRING) type Query{representation:Token note(arg:String="@chigiri_mock_scalar(representation:STRING)"):String}"#
    )
    .is_ok());
}

#[test]
fn rep_02_03_new_scalar_vocabulary_rejects_invalid_shape_and_accepts_equivalence() {
    let equivalent = "directive @representation(maxItems:Int,minItems:Int,maxBytes:Int,minBytes:Int,max:Float,min:Float,type:ChigiriMockRepresentation!) on SCALAR scalar Token @representation(type:STRING) type Query{x:Token}";
    assert!(parse(equivalent).is_ok());
    for sdl in [
        "scalar Token @representation(representation:STRING) type Query{x:Token}",
        "scalar Token @representation(type:null) type Query{x:Token}",
        "scalar Token @representation type Query{x:Token}",
        "scalar Token @representation(type:DATE) type Query{x:Token}",
        "scalar Token @representation(type:STRING,other:1) type Query{x:Token}",
        "scalar Token @representation(type:STRING,type:STRING) type Query{x:Token}",
        "directive @representation(type:String!) on SCALAR type Query{x:Int}",
        "directive @representation(type:ChigiriMockRepresentation!) on FIELD_DEFINITION type Query{x:Int}",
        "directive @representation(type:ChigiriMockRepresentation!) on SCALAR directive @representation(type:ChigiriMockRepresentation!) on SCALAR type Query{x:Int}",
        "type Query{x:Int @representation(type:STRING)}",
    ] {
        assert!(parse(sdl).is_err(), "{sdl}");
    }
}
