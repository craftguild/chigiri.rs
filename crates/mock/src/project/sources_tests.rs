//! Verify adopted authority, explicit acquisition, and confined storage inputs.

use std::fs;

use super::test_support::Fixture;
use super::{
    SchemaSyncStatus, codegen_project, compare_public_schema, read_project_schema, update_project,
};

const SDL: &str = "type Query { answer: Int! }";

#[test]
fn as_03_internal_schema_never_requires_or_observes_external_public_sdl() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let adopted = read_project_schema(&fixture.project).unwrap();
    let digest = adopted.ir().graph().source().digest();

    fs::write(fixture.base.join("schema.graphql"), "invalid external SDL").unwrap();
    let changed = read_project_schema(&fixture.project).unwrap();
    assert_eq!(changed.ir().graph().source().digest(), digest);
    assert!(
        codegen_project(&fixture.project)
            .unwrap()
            .events
            .iter()
            .all(|event| {
                event.action == super::UpdateAction::Unchanged
                    || event.action == super::UpdateAction::SeedKept
            })
    );

    fs::remove_file(fixture.base.join("schema.graphql")).unwrap();
    assert!(read_project_schema(&fixture.project).is_ok());
    assert!(codegen_project(&fixture.project).is_ok());
    assert_eq!(
        compare_public_schema(&fixture.project).status,
        SchemaSyncStatus::Unavailable
    );
}

#[test]
fn as_01_05_update_adopts_ordered_logical_sources_from_relative_and_absolute_locators() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let extra = fixture.base.join("extra.graphql");
    fs::write(&extra, "extend type Query { other: Boolean }").unwrap();
    replace_sources(
        &fixture,
        &format!(
            "{{\"../schema.graphql\", {}}}",
            serde_json::to_string(extra.to_str().unwrap()).unwrap()
        ),
    );

    update_project(&fixture.project).unwrap();
    let input = read_project_schema(&fixture.project).unwrap();
    let sources = input.ir().public_sources();
    assert_eq!(sources[0].name(), "schema/001.graphql");
    assert_eq!(sources[1].name(), "schema/002.graphql");
    assert_eq!(sources[0].text(), "type Query { answer : Int ! }\n");
    assert_eq!(sources[1].text(), "extend type Query { other : Boolean }\n");
}

#[test]
fn as_09_external_duplicates_and_budgets_fail_without_physical_path_disclosure() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    fs::hard_link(
        fixture.base.join("schema.graphql"),
        fixture.base.join("alias.graphql"),
    )
    .unwrap();
    replace_sources(&fixture, "{\"../schema.graphql\", \"../alias.graphql\"}");
    let error = update_project(&fixture.project).err().unwrap();
    assert_eq!(error.message, "duplicate SDL source");

    replace_sources(&fixture, "{\"../schema.graphql\"}");
    let oversized = format!("{SDL}\n#{}", "x".repeat(crate::schema::MAX_SDL_BYTES));
    fs::write(fixture.base.join("schema.graphql"), oversized).unwrap();
    let error = update_project(&fixture.project).err().unwrap();
    assert_eq!(error.phase, "limit");
    assert_eq!(error.file, "schema/001.graphql");
    assert!(!error.to_string().contains(fixture.base.to_str().unwrap()));
}

#[test]
fn as_09_storage_requires_portable_project_relative_no_follow_paths() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    fs::write(
        fixture.project.join("storage.graphql"),
        "# private storage\n",
    )
    .unwrap();
    fs::write(fixture.base.join("outside.graphql"), "# outside").unwrap();
    for path in [
        "../outside.graphql",
        "/tmp/outside.graphql",
        "a/../storage.graphql",
    ] {
        replace_storage(&fixture, path);
        let error = codegen_project(&fixture.project).err().unwrap();
        assert_eq!(error.message, "explicit storage SDL paths required");
    }

    replace_storage(&fixture, "storage-link.graphql");
    std::os::unix::fs::symlink(
        fixture.project.join("storage.graphql"),
        fixture.project.join("storage-link.graphql"),
    )
    .unwrap();
    assert!(codegen_project(&fixture.project).is_err());

    fs::remove_file(fixture.project.join("storage-link.graphql")).unwrap();
    fs::create_dir(fixture.base.join("storage-outside")).unwrap();
    fs::write(
        fixture.base.join("storage-outside/private.graphql"),
        "# private",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        fixture.base.join("storage-outside"),
        fixture.project.join("storage-dir"),
    )
    .unwrap();
    replace_storage(&fixture, "storage-dir/private.graphql");
    assert!(codegen_project(&fixture.project).is_err());

    fs::hard_link(
        fixture.project.join("storage.graphql"),
        fixture.project.join("storage-alias.graphql"),
    )
    .unwrap();
    replace_storage_list(&fixture, "{\"storage.graphql\", \"storage-alias.graphql\"}");
    assert_eq!(
        codegen_project(&fixture.project).err().unwrap().message,
        "duplicate SDL source"
    );
}

#[test]
fn as_07_project_reference_reader_ignores_resolver_and_seed_bodies() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new("type User { id: ID! } type Query { users: [User!]! @virtual }");
    let resolver = fixture.project.join("resolvers/query/users.luau");
    let seed = fixture.project.join("seeds/User.jsonl");
    fs::remove_file(&resolver).unwrap();
    fs::remove_file(&seed).unwrap();
    fifo(&resolver);
    fifo(&seed);

    let schema = read_project_schema(&fixture.project).unwrap();
    assert_eq!(schema.ir().scripts().len(), 1);
    assert_eq!(schema.ir().models().len(), 1);
}

#[test]
fn as_06_project_reference_rechecks_generation_and_transaction_before_publication() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let stale = read_project_schema(&fixture.project).unwrap();
    fs::write(
        fixture.base.join("schema.graphql"),
        "\"Changed\" type Query { answer: Int! }",
    )
    .unwrap();
    update_project(&fixture.project).unwrap();

    assert!(stale.verify_unchanged().is_err());

    let current = read_project_schema(&fixture.project).unwrap();
    let marker = fixture.project.join(super::filesystem::TRANSACTION);
    fs::write(&marker, "reference race").unwrap();
    let error = current.verify_unchanged().err().unwrap();
    assert_eq!(error.message, "unfinished project transaction");
    fs::remove_file(marker).unwrap();
}

fn replace_sources(fixture: &Fixture, list: &str) {
    let config = fixture.read("mock.luau");
    let start = config.find("schema = {sources = {").unwrap();
    let values = start + "schema = {sources = ".len();
    let end = config[values..].find("},").unwrap() + values;
    fixture.write(
        "mock.luau",
        &format!("{}{}{}", &config[..values], list, &config[end..]),
    );
}

fn replace_storage(fixture: &Fixture, path: &str) {
    replace_storage_list(
        fixture,
        &format!("{{{}}}", serde_json::to_string(path).unwrap()),
    );
}

fn replace_storage_list(fixture: &Fixture, list: &str) {
    let config = fixture.read("mock.luau");
    let start = config.find("storage = {sources = {").unwrap();
    let values = start + "storage = {sources = ".len();
    let end = config[values..].find("},").unwrap() + values;
    fixture.write(
        "mock.luau",
        &format!("{}{}{}", &config[..values], list, &config[end..]),
    );
}

fn fifo(path: &std::path::Path) {
    assert!(
        std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}
