use std::fs;

use super::simplified_tests::files;
use super::test_support::Fixture;
use super::{SchemaSyncStatus, compare_public_schema};

const SDL: &str = "\"Query docs\" type Query { answer: Int! }";

#[test]
fn as_02_07_comparison_ignores_lexical_drift_but_observes_description_drift() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let first = compare_public_schema(&fixture.project);
    assert_eq!(first.status, SchemaSyncStatus::InSync);
    assert_eq!(first.adopted_digest, first.input_digest);
    assert!(first.diagnostics.is_empty());

    fs::write(
        fixture.base.join("schema.graphql"),
        "# ignored\r\n\"Query docs\" type Query, { answer: Int! }\r\n",
    )
    .unwrap();
    assert_eq!(
        compare_public_schema(&fixture.project).status,
        SchemaSyncStatus::InSync
    );

    fs::write(
        fixture.base.join("schema.graphql"),
        "\"Changed docs\" type Query { answer: Int! }",
    )
    .unwrap();
    let changed = compare_public_schema(&fixture.project);
    assert_eq!(changed.status, SchemaSyncStatus::UpdateRequired);
    assert_ne!(changed.adopted_digest, changed.input_digest);
    assert!(changed.diagnostics.is_empty());
}

#[test]
fn as_07_comparison_skips_private_schema_modules_seeds_and_all_writes() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let config = fixture.read("mock.luau").replace(
        "storage = {sources = {}}",
        "storage = {sources = {\"missing.graphql\"}}",
    );
    fixture.write("mock.luau", &config);
    let before = files(&fixture.project);

    let report = compare_public_schema(&fixture.project);
    assert_eq!(report.status, SchemaSyncStatus::InSync);
    assert_eq!(files(&fixture.project), before);
}

#[test]
fn as_07_unavailable_never_reports_synchronized_and_digests_are_verified_side_only() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    fs::write(fixture.base.join("schema.graphql"), "invalid SDL").unwrap();
    let unavailable = compare_public_schema(&fixture.project);
    assert_eq!(unavailable.status, SchemaSyncStatus::Unavailable);
    assert!(unavailable.adopted_digest.is_some());
    assert!(unavailable.input_digest.is_none());
    assert_eq!(unavailable.diagnostics.len(), 1);

    fixture.write("generated/update-state.json", "{}");
    let corrupted = compare_public_schema(&fixture.project);
    assert_eq!(corrupted.status, SchemaSyncStatus::Unavailable);
    assert!(corrupted.adopted_digest.is_none());
    assert!(corrupted.input_digest.is_none());

    let fresh = Fixture::new(SDL);
    let path = "generated/schema.snapshot.json";
    let snapshot = fresh.read(path);
    let digest =
        serde_json::from_str::<serde_json::Value>(&snapshot).unwrap()["public_schema"]["digest"]
            .as_str()
            .unwrap()
            .to_owned();
    fresh.write(path, &snapshot.replace(&digest, &"0".repeat(64)));
    let corrupted = compare_public_schema(&fresh.project);
    assert_eq!(corrupted.status, SchemaSyncStatus::Unavailable);
    assert!(corrupted.adopted_digest.is_none());
    assert!(corrupted.input_digest.is_none());
}
