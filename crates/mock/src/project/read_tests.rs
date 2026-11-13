//! Verifies directory readiness, generated ownership, start/end races, and
//! private diagnostics.

use std::fs;

use chigiri::core::Source;

use super::filesystem::Directory;
use super::read_project;
use super::test_support::Fixture;
use crate::ir::MockIr;

const BLOG: &str = include_str!("../../fixtures/blog.graphql");

#[test]
fn mp_08_14_16_readiness_uses_active_edited_modules_and_owned_initial_bytes() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    let module = "resolvers/query/getUsers.luau";
    let original = fixture.read(module);
    fixture.write(module, &format!("-- User migration\n{original}"));
    fixture.write("seeds/User.jsonl", "{\"id\":\"u1\",\"name\":\"A\"}\r\n");
    fixture.write("resolvers/query/getUsers.luau.dist", "invalid proposal");
    fixture.write("resolvers/query/removed.luau", "invalid orphan");
    let checked = read_project(&fixture.project).unwrap();
    let bundle = checked.bundle().to_owned();
    assert!(bundle.contains("User migration"));
    assert!(!bundle.contains("invalid proposal") && !bundle.contains("invalid orphan"));
    fixture.write(module, "invalid source after check");
    fixture.write("seeds/User.jsonl", "invalid seed after check");
    assert_eq!(checked.bundle(), bundle);
    assert!(read_project(&fixture.project).is_err());
}

#[test]
fn mp_13_16_readiness_rejects_missing_active_input_drift_generation_and_unfinished_update() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    for (path, changed) in [
        ("generated/types.d.luau", "edited"),
        ("generated/update-state.json", "{}"),
        ("generated/schema.snapshot.json", "{}"),
        ("seeds/User.jsonl", "missing"),
        ("resolvers/query/getUsers.luau", "missing"),
    ] {
        let old = fixture.read(path);
        if changed == "missing" {
            fs::remove_file(fixture.project.join(path)).unwrap();
        } else {
            fixture.write(path, changed);
        }
        assert_eq!(
            read_project(&fixture.project).err().unwrap().exit,
            1,
            "{path}"
        );
        fixture.write(path, &old);
    }
    fixture.write(".chigiri-mock-transaction", "incomplete");
    assert_eq!(
        read_project(&fixture.project).err().unwrap().message,
        "unfinished project transaction"
    );
    fs::remove_file(fixture.project.join(".chigiri-mock-transaction")).unwrap();
    fs::write(fixture.base.join("schema.graphql"), "invalid external SDL").unwrap();
    assert!(read_project(&fixture.project).is_ok());
    fs::remove_file(fixture.base.join("schema.graphql")).unwrap();
    assert!(read_project(&fixture.project).is_ok());
}

#[test]
fn mp_15_16_seed_errors_show_logical_model_field_line_without_values_or_physical_paths() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    fixture.write(
        "seeds/User.jsonl",
        "{\"id\":\"sensitive-id\",\"name\":\"first\"}\n{\"id\":\"secret-value\"}\n",
    );
    let error = read_project(&fixture.project).err().unwrap();
    assert_eq!(error.file, "seeds/User.jsonl");
    assert_eq!(error.position, (2, 1));
    assert_eq!(error.coordinate.as_ref(), "User.name");
    let display = error.to_string();
    for secret in [
        "sensitive-id",
        "secret-value",
        fixture.base.to_str().unwrap(),
    ] {
        assert!(!display.contains(secret));
    }
}

#[test]
fn mp_13_16_checked_snapshot_rejects_internal_input_changes_but_ignores_external_changes() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    for path in [
        "mock.luau",
        "generated/update-state.json",
        "generated/schema.snapshot.json",
        "generated/types.d.luau",
        "resolvers/query/getUsers.luau",
        "seeds/User.jsonl",
    ] {
        let old = fixture.read(path);
        let result = super::read::read_with(&fixture.project, || {
            fixture.write(path, "changed after check")
        });
        assert!(result.is_err(), "{path}");
        fixture.write(path, &old);
    }
    let external = fixture.read("../schema.graphql");
    assert!(
        super::read::read_with(&fixture.project, || fixture
            .write("../schema.graphql", "changed after check"))
        .is_ok()
    );
    fixture.write("../schema.graphql", &external);
    assert!(
        super::read::read_with(&fixture.project, || fixture
            .write(".chigiri-mock-transaction", "started"))
        .is_err()
    );
}

#[test]
fn mp_13_17_project_lock_excludes_writers_and_releases_without_waiting() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    let directory = Directory::open(&fixture.project).unwrap();
    let first = directory.lock(false).unwrap();
    let second = directory.lock(false).unwrap();
    assert!(directory.lock(true).is_err());
    drop(first);
    drop(second);
    let writer = directory.lock(true).unwrap();
    assert!(read_project(&fixture.project).is_err());
    assert!(directory.lock(true).is_err());
    drop(writer);
    assert!(read_project(&fixture.project).is_ok());
}

#[test]
fn mp_16_relation_names_reject_controls_and_long_seed_diagnostics_stay_bounded() {
    let _test_case = crate::vm_gate::test_case();
    let source = "type Item { id:ID! peer:Item! @relation(fields:[\"line\\nfield\"],references:[\"id\"]) } type Query { items:[Item!]! }";
    let error = MockIr::parse(vec![Source::new("schema.graphql", source)])
        .err()
        .unwrap();
    assert_eq!(error.code, "invalid relation component names");
    let span = error.span.unwrap();
    assert_eq!(span.source_id.index(), 0);
    assert_eq!(span.start, source.find("@relation").unwrap());
    assert!(span.end > span.start);
    // The 256-byte relation-key limit is independent of GraphQL's ASCII field-name grammar.
    let key = "x".repeat(1024);
    let source = format!(
        "type Item {{ id:ID! peer:Item! @relation(fields:[\"{key}\"],references:[\"id\"]) }} type Query {{ items:[Item!]! }}"
    );
    assert!(MockIr::parse(vec![Source::new("schema.graphql", &source)]).is_ok());
    let fixture = Fixture::new(&source);
    fixture.write("seeds/Item.jsonl", "{\"id\":\"private-id\"}\n");
    let output = read_project(&fixture.project).err().unwrap().to_string();
    assert_eq!(output.lines().count(), 1);
    assert!(output.len() < 300);
    assert!(!output.contains("private-id"));
    assert!(output.contains("..."));
}
