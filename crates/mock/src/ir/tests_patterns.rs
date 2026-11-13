//! Test regex limits, sharing, and the supported SDL pattern dialect.

use chigiri::core::Source;
use std::fmt::Write as _;
use std::sync::Arc;

use super::SchemaGraph;

fn parse(pattern: &str) -> Result<SchemaGraph, crate::schema::SchemaError> {
    let quoted = serde_json::to_string(pattern).unwrap();
    SchemaGraph::parse(vec![Source::new(
        "regex.graphql",
        format!("type Query{{x:String @constraint(pattern:{quoted})}}"),
    )])
}

#[test]
fn sd_14_regex_dialect_and_per_source_construction_limits() {
    for pattern in ["", "^\\p{Greek}+$", "(?i)abc", "a|b", "a{1024}"] {
        assert!(parse(pattern).is_ok(), "{pattern}");
    }
    for pattern in ["[", "(?=a)", "(?!a)", "(?<=a)b", "(a)\\1", "a{1000000}"] {
        let error = parse(pattern).err().unwrap();
        assert_eq!(error.code, "invalid or oversized regex");
        assert!(error.span.is_some());
    }
    assert!(parse(&"a".repeat(1024)).is_ok());
    assert_eq!(
        parse(&"a".repeat(1025)).err().unwrap().code,
        "regex source limit"
    );
    assert!(parse(&format!("{}a{}", "(".repeat(65), ")".repeat(65))).is_err());
}

#[test]
fn sd_14_schema_interning_counts_unique_sources_across_all_locations() {
    let mut fields = String::new();
    for i in 0..16 {
        write!(
            fields,
            "f{i}(x:String @constraint(pattern:\"p{i}\")):String @constraint(pattern:\"p{i}\") "
        )
        .unwrap();
    }
    let source = format!("input I{{x:String @constraint(pattern:\"p0\")}} type Query{{{fields}}}");
    let graph = SchemaGraph::parse(vec![Source::new("regex", source)]).unwrap();
    let query = &graph.types()[graph.query().0];
    for field in query.fields.values() {
        let field = &graph.fields()[field.0];
        let input = &graph.inputs()[field.arguments[0].0];
        assert!(Arc::ptr_eq(
            field.constraints.pattern.as_ref().unwrap(),
            input.constraints.pattern.as_ref().unwrap()
        ));
    }
    let source = format!("type Query{{{fields} extra:String @constraint(pattern:\"seventeen\")}}");
    assert_eq!(
        SchemaGraph::parse(vec![Source::new("regex", source)])
            .err()
            .unwrap()
            .code,
        "schema regex reservation limit"
    );
}

#[test]
fn sd_14_matching_keeps_unicode_flags_substrings_and_explicit_anchors() {
    for (pattern, text, expected) in [
        ("abc", "xabcx", true),
        ("^abc$", "xabcx", false),
        ("abc", "ABC", false),
        ("(?i)abc", "ABC", true),
        ("^\\p{Greek}+$", "Ελληνικά", true),
        ("^\\p{Greek}+$", "abc", false),
        ("/abc/", "abc", false),
        ("/abc/", "/abc/", true),
    ] {
        let graph = parse(pattern).unwrap();
        let field = graph.types()[graph.query().0].fields["x"];
        let regex = graph.fields()[field.0]
            .constraints
            .pattern
            .as_ref()
            .unwrap();
        assert_eq!(regex.is_match(text, || Ok(())).unwrap(), expected);
    }
}
