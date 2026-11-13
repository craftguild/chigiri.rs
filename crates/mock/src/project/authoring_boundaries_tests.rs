//! ADR-0047 canonical update and generated-identity acceptance.

use super::state::State;
use super::test_support::Fixture;
use super::update_project;

fn state(fixture: &Fixture) -> State {
    serde_json::from_str(&fixture.read("generated/update-state.json")).unwrap()
}

fn declaration<'a>(types: &'a str, name: &str) -> &'a str {
    let marker = format!("export type {name}");
    let start = types.find(&marker).unwrap();
    let rest = &types[start..];
    let end = rest[marker.len()..]
        .find("\nexport type ")
        .map_or(rest.len(), |offset| marker.len() + offset);
    &rest[..end]
}

#[test]
fn ab_12_15_current_update_preserves_sources_and_regenerates_correct_record_identity() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new("type Account{id:ID!} type Query{accounts:[Account!]!}");
    let module = "resolvers/query/accounts.luau";
    let resolver = format!("-- user-owned resolver\n{}", fixture.read(module));
    let seed = "{\"id\":\"a1\"}\n";
    fixture.write(module, &resolver);
    fixture.write("seeds/Account.jsonl", seed);
    fixture.write("storage.graphql", "extend type Account{balance:Int!}");
    let config = fixture.read("mock.luau").replace(
        "storage = {sources = {}}",
        "storage = {sources = {\"storage.graphql\"}}",
    );
    fixture.write("mock.luau", &config);
    let before = state(&fixture).generation;

    let report = update_project(&fixture.project).unwrap();
    assert!(report.schema_changed);
    assert_ne!(state(&fixture).generation, before);
    assert_eq!(fixture.read("mock.luau"), config);
    assert_eq!(
        fixture.read("storage.graphql"),
        "extend type Account{balance:Int!}"
    );
    assert_eq!(fixture.read(module), resolver);
    assert_eq!(fixture.read("seeds/Account.jsonl"), seed);
    let types = fixture.read("generated/types.d.luau");
    let stored = declaration(&types, "ChigiriAccountStored");
    let record = declaration(&types, "ChigiriAccountRecord");
    assert_eq!(stored.matches("[\"balance\"]").count(), 1, "{stored}");
    assert_eq!(record.matches("[\"balance\"]").count(), 0, "{record}");
    assert!(record.contains("ChigiriAccountStored & {}"), "{record}");

    let placed = state(&fixture).generation;
    let second = update_project(&fixture.project).unwrap();
    assert!(!second.schema_changed);
    assert_eq!(state(&fixture).generation, placed);
    assert_eq!(fixture.read(module), resolver);
    assert_eq!(fixture.read("seeds/Account.jsonl"), seed);
}
