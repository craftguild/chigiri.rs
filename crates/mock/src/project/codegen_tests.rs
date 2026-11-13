use std::fs;

use super::simplified_tests::files;
use super::test_support::Fixture;
use super::{codegen_project, read_project_schema, update_project};

const SDL: &str = "type User { id: ID! } type Query { users: [User!]! }";

#[test]
fn as_04_local_schema_drift_requires_codegen_and_preserves_authored_bodies() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    fixture.write("storage.graphql", "extend type User { note: String }\n");
    enable_storage(&fixture);
    fixture.write("resolvers/query/users.luau", "invalid authored resolver");
    fixture.write("seeds/User.jsonl", "invalid authored seed");

    let error = read_project_schema(&fixture.project).err().unwrap();
    assert!(error.message.contains("chigiri mock codegen"));
    codegen_project(&fixture.project).unwrap();
    assert_eq!(
        fixture.read("resolvers/query/users.luau"),
        "invalid authored resolver"
    );
    assert_eq!(fixture.read("seeds/User.jsonl"), "invalid authored seed");
    assert!(read_project_schema(&fixture.project).is_ok());
}

#[test]
fn as_05_joint_public_and_private_change_requires_update_not_codegen() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    fixture.write(
        "storage.graphql",
        "extend type User { secret: SecretCode }\n",
    );
    enable_storage(&fixture);
    fs::write(
        fixture.base.join("schema.graphql"),
        format!("enum SecretCode {{ VALUE }}\n{SDL}"),
    )
    .unwrap();

    assert!(codegen_project(&fixture.project).is_err());
    update_project(&fixture.project).unwrap();
    let schema = read_project_schema(&fixture.project).unwrap();
    assert!(schema.ir().graph().type_id("SecretCode").is_some());
    assert!(
        schema
            .ir()
            .graph()
            .types()
            .iter()
            .any(|ty| ty.name.as_ref() == "User" && ty.fields.contains_key("secret"))
    );
}

#[test]
fn as_05_identical_codegen_does_not_churn_generation_or_bytes() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let before = files(&fixture.project);

    let report = codegen_project(&fixture.project).unwrap();
    assert_eq!(files(&fixture.project), before);
    assert!(report.events.iter().all(|event| {
        matches!(
            event.action,
            super::UpdateAction::Unchanged | super::UpdateAction::SeedKept
        )
    }));
}

#[test]
fn as_05_description_only_update_adopts_without_claiming_no_schema_change() {
    let _case = crate::vm_gate::test_case();
    let before_sdl = "\"Before\" type Query { answer: Int! }";
    let fixture = Fixture::new(before_sdl);
    let before_types = fixture.read("generated/types.d.luau");
    let before_state: serde_json::Value =
        serde_json::from_str(&fixture.read("generated/update-state.json")).unwrap();
    fs::write(
        fixture.base.join("schema.graphql"),
        "\"After\" type Query { answer: Int! }",
    )
    .unwrap();

    let report = update_project(&fixture.project).unwrap();
    assert!(report.schema_changed);
    assert_eq!(fixture.read("generated/types.d.luau"), before_types);
    let after_state: serde_json::Value =
        serde_json::from_str(&fixture.read("generated/update-state.json")).unwrap();
    assert_ne!(after_state["public_digest"], before_state["public_digest"]);
    assert_ne!(after_state["generation"], before_state["generation"]);
    assert!(
        read_project_schema(&fixture.project)
            .unwrap()
            .ir()
            .public_sources()[0]
            .text()
            .contains("After")
    );
}

fn enable_storage(fixture: &Fixture) {
    let config = fixture.read("mock.luau").replace(
        "storage = {sources = {}}",
        "storage = {sources = {\"storage.graphql\"}}",
    );
    fixture.write("mock.luau", &config);
}
