//! Verify canonical paths and reject reserved names, case collisions, and parent conflicts before placement.

use std::collections::BTreeMap;

use chigiri::core::Source;

use super::Layout;
use crate::ir::MockIr;

fn ir(source: &str) -> MockIr {
    MockIr::parse(vec![Source::new("schema/001.graphql", source)]).unwrap()
}

#[test]
fn mp_01_02_layout_uses_roles_and_required_fields_in_two_domains() {
    for (source, model, query) in [
        (
            include_str!("../../fixtures/blog.graphql").to_owned(),
            "User",
            "getUsers",
        ),
        (
            include_str!("../../fixtures/blog.graphql")
                .replace("User", "Member")
                .replace("Query", "Lookup")
                .replace("Mutation", "Commands")
                + "\nschema { query: Lookup mutation: Commands }",
            "Member",
            "getMembers",
        ),
    ] {
        let layout = Layout::new(&ir(&source)).unwrap();
        assert_eq!(layout.resolvers.len(), 6);
        assert_eq!(layout.seeds.len(), 2);
        assert!(
            layout
                .resolvers
                .iter()
                .any(|entry| entry.path.as_str() == format!("resolvers/query/{query}.luau"))
        );
        assert!(
            layout.resolvers.iter().any(
                |entry| entry.path.as_str() == format!("resolvers/mutation/create{model}.luau")
            )
        );
        assert!(
            layout
                .seeds
                .iter()
                .any(|entry| entry.path.as_str() == format!("seeds/{model}.jsonl"))
        );
        assert!(
            layout
                .resolvers
                .iter()
                .all(|entry| !entry.coordinate.ends_with(".author")
                    && !entry.coordinate.ends_with(".posts"))
        );
    }
    let schema =
        ir("type Item { id: ID! label: String! } type Query { item: Item! nested: Query! }");
    let layout = Layout::new(&schema).unwrap();
    assert_eq!(layout.resolvers.len(), 2);
    assert!(
        layout
            .resolvers
            .iter()
            .all(|entry| { !entry.path.starts_with("resolvers/objects/Item/") })
    );
    assert_eq!(
        layout
            .resolvers
            .iter()
            .filter(|entry| entry.coordinate == "Query.item")
            .count(),
        1
    );
}

#[test]
fn mp_02_layout_keeps_existing_paths_when_ir_positions_move() {
    let base = "type Query { z: String! }";
    let expanded = "type A { value: String! } type Query { a: A! z: String! }";
    let paths = |s| {
        Layout::new(&ir(s))
            .unwrap()
            .resolvers
            .into_iter()
            .map(|entry| (entry.coordinate, entry.path))
            .collect::<BTreeMap<_, _>>()
    };
    for (coordinate, path) in paths(base) {
        assert_eq!(paths(expanded)[&coordinate], path);
    }
}

#[test]
fn mp_02_layout_rejects_case_parent_reserved_and_component_length_collisions() {
    for source in [
        "type Query { a: String! @chigiri_mock_resolver A: String! @chigiri_mock_resolver }".to_owned(),
        "type Item { a: Int! @chigiri_mock_resolver } type item { b: Int! @chigiri_mock_resolver } type Query { x: Item! @chigiri_mock_resolver y: item! @chigiri_mock_resolver }".to_owned(),
        "type Query { con: String! @chigiri_mock_resolver }".to_owned(),
        "type NUL @chigiri_mock_model { id: ID! } type Query { n: NUL! @chigiri_mock_resolver }".to_owned(),
        format!("type Query {{ {}: Int! @chigiri_mock_resolver }}", "x".repeat(247)),
    ] {
        // Legacy directives are rejected during schema parsing before layout construction.
        assert!(MockIr::parse(vec![Source::new("schema/001.graphql", &source)]).is_err());
    }
    Layout::new(&ir("type End { local: Int! } type Query { while: End! }")).unwrap();
}
