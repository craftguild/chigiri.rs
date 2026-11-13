//! Verifies construction failures and source locations for owned SDL.

use chigiri::Value;
use chigiri::core::Source;
use chigiri::experimental::OwnedSchema;

use super::support::{Dispatch, schema};

#[test]
fn rsm_20_bind_rejects_wrong_instance_missing_duplicate_and_unknown_fields() {
    let source = schema("type Query { item: Item! } type Item { value: Int! }");
    let other = schema("type Query { item: Item! } type Item { value: Int! }");
    assert!(
        source
            .clone()
            .bind(Dispatch::new(&other, "Item", Value::Int(1)))
            .is_err()
    );
    for change in 0..3 {
        let mut dispatch = Dispatch::new(&source, "Item", Value::Int(1));
        match change {
            0 => {
                dispatch.coordinates.pop();
            }
            1 => {
                dispatch.coordinates[0] = dispatch.coordinates[1].clone();
            }
            _ => {
                dispatch.coordinates[0].1 = "unknown".into();
            }
        }
        assert!(source.clone().bind(dispatch).is_err());
    }
    let scalar = schema("scalar Date type Query { value: Date }");
    assert!(
        scalar
            .clone()
            .bind(Dispatch::new(&scalar, "Query", Value::Null))
            .is_err()
    );
}

#[test]
fn rsm_20_composition_retains_source_positions_and_accepts_subscription_roots() {
    let source = OwnedSchema::parse(vec![
        Source::new("models.graphql", "type Item { value: Int! }"),
        Source::new(
            "roots.graphql",
            "type Query { item: Item! } extend type Item { other: String }",
        ),
    ])
    .unwrap();
    let field = source
        .semantic()
        .type_by_name("Item")
        .unwrap()
        .field("other")
        .unwrap();
    assert_eq!(field.span.source_id.index(), 1);
    assert_eq!(
        source.sources()[1].location(field.span.start).unwrap().line,
        1
    );
    let subscription = OwnedSchema::parse(vec![Source::new(
        "subscription.graphql",
        "type Query { value: Int } type Subscription { value: Int }",
    )])
    .unwrap();
    assert_eq!(
        subscription
            .semantic()
            .roots()
            .subscription()
            .unwrap()
            .type_name,
        "Subscription"
    );
    for invalid in [
        "type Query { value: Missing }",
        "type Query { value(input: Choice): Int } input Choice @oneOf { a: Int! }",
        "type Query { value(input: Choice): Int } input Choice { a: Choice! }",
    ] {
        let error = OwnedSchema::parse(vec![Source::new("bad.graphql", invalid)])
            .err()
            .unwrap();
        assert!(!error.diagnostics().unwrap().is_empty());
        assert_eq!(format!("{error:?}"), "invalid owned schema");
    }
}

#[test]
fn rsm_20_owned_digest_matches_generator_length_prefixed_source_hash() {
    let source = OwnedSchema::parse(vec![
        Source::new("a.graphql", "type Query { value: Int }\n"),
        Source::new("nested/b.gql", "scalar Date\n"),
    ])
    .unwrap();
    assert_eq!(
        source.digest().to_hex(),
        "cccb9d5cb8a2598906514a939c9fe268a76502e3dab4e7bd35262c5bef9d121b"
    );
}

#[test]
fn rsm_20_owned_source_budget_is_aggregate_and_exact() {
    let prefix = "type Query { value: Int } #";
    let exact = format!("{prefix}{}", "a".repeat(256 * 1024 - prefix.len()));
    assert!(OwnedSchema::parse(vec![Source::new("exact", &exact)]).is_ok());
    assert!(OwnedSchema::parse(vec![Source::new("over", exact + "a")]).is_err());
    assert!(
        OwnedSchema::parse(vec![
            Source::new("same", "type Query { value: Int }"),
            Source::new("same", "scalar Other")
        ])
        .is_err()
    );
    assert!(OwnedSchema::parse(vec![Source::new("../escape", prefix)]).is_err());
    assert!(OwnedSchema::parse(vec![]).is_err());
    let mut sources = vec![Source::new("root", "type Query { value: Int }")];
    for index in 0..64 {
        sources.push(Source::new(
            format!("part{index}"),
            format!("scalar S{index}"),
        ));
    }
    assert!(OwnedSchema::parse(sources.clone()).is_ok());
    sources.push(Source::new("extra", "# extra"));
    assert!(OwnedSchema::parse(sources).is_err());
}

#[test]
fn rsm_20_comment_only_composition_parts_keep_digest_and_source_indices() {
    let root = "type Query { value: Int }";
    let source = OwnedSchema::parse(vec![
        Source::new("empty", "# Vocabulary already supplied\n"),
        Source::new("root", root),
    ])
    .unwrap();
    assert_eq!(source.sources().len(), 2);
    assert_eq!(
        source
            .semantic()
            .type_by_name("Query")
            .unwrap()
            .name_span()
            .unwrap()
            .source_id
            .index(),
        1
    );
    assert_ne!(
        source.digest(),
        OwnedSchema::parse(vec![Source::new("root", root)])
            .unwrap()
            .digest()
    );
    assert!(OwnedSchema::parse(vec![Source::new("empty", "# Empty\n")]).is_err());
    assert!(
        OwnedSchema::parse(vec![Source::new("root", root), Source::new("invalid", "?")]).is_err()
    );
}

#[test]
fn rsm_20_owned_objects_share_long_typenames_within_the_schema_instance() {
    let name = "T".repeat(10_000);
    let source = schema(&format!(
        "type Query {{ item: {name}! }} type {name} {{ value: Int! }}"
    ));
    let first = source.object(&name, 1).unwrap();
    let clone = source.clone();
    for value in 0..1000 {
        let next = clone.object(&name, value).unwrap();
        assert_eq!(first.type_name(), next.type_name());
        assert_eq!(first.type_name().as_ptr(), next.type_name().as_ptr());
    }
    drop(source);
    drop(clone);
    assert_eq!(first.type_name(), name);
}
