//! Verifies the difference between the logical input ownership snapshot and
//! full project readiness using real files.

use std::fs;

use super::filesystem::Directory;
use super::test_support::Fixture;
use super::{ActiveKind, ActiveSnapshot, read_project};

const SDL: &str = include_str!("../../fixtures/blog.graphql");

#[test]
fn ai_15_snapshot_accepts_scaffolds_but_rejects_invalid_modules_and_invalid_seed() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new("type Query { manual: String! @chigiri_mock_resolver }");
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    snapshot.check().unwrap();
    fixture.write("resolvers/query/manual.luau", "not valid Luau !!");
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    assert_eq!(snapshot.files()[0].source(), "not valid Luau !!");
    assert!(snapshot.check().is_err());
    let fixture = Fixture::new(SDL);
    fixture.write("seeds/User.jsonl", "{broken JSON\n");
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    assert!(
        snapshot
            .files()
            .iter()
            .any(|f| matches!(f.kind(), ActiveKind::Seed(_)) && f.source() == "{broken JSON\n")
    );
    assert!(snapshot.check().is_err());
    assert!(read_project(&fixture.project).is_err());
}

#[test]
fn ai_02_03_active_inventory_includes_ready_edited_nonempty_but_excludes_dist_and_orphan() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    fixture.write(
        "resolvers/query/getUsers.luau",
        "-- Work in progress\ninvalid",
    );
    fixture.write("seeds/User.jsonl", "{\"id\":\"u\",\"name\":\"A\"}\n");
    fixture.write("resolvers/query/getUsers.luau.dist", "must not be opened");
    fixture.write("resolvers/query/orphan.luau", "must not be opened");
    fixture.write(
        ".chigiri-mock/inference/old/candidate",
        "must not be opened",
    );
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    assert_eq!(snapshot.files().len(), 8);
    assert!(
        snapshot
            .files()
            .iter()
            .all(|file| !file.path().ends_with("dist")
                && !file.path().contains("orphan")
                && !file.path().contains("inference"))
    );
    assert!(
        snapshot
            .files()
            .iter()
            .any(|file| file.coordinate() == "Query.getUsers"
                && file.source().contains("Work in progress"))
    );
    assert!(
        snapshot
            .files()
            .iter()
            .any(|file| file.coordinate() == "User" && !file.source().is_empty())
    );
    assert!(
        snapshot
            .files()
            .iter()
            .any(|file| file.coordinate() == "Mutation.createUser"
                && file.source().contains("NOT_IMPLEMENTED"))
    );
}

#[test]
fn ai_15_snapshot_refuses_missing_ownership_schema_type_and_transaction_drift() {
    let _test_case = crate::vm_gate::test_case();
    for path in [
        "generated/update-state.json",
        "generated/types.d.luau",
        "resolvers/query/getUsers.luau",
        "seeds/User.jsonl",
    ] {
        let fixture = Fixture::new(SDL);
        fs::remove_file(fixture.project.join(path)).unwrap();
        assert!(ActiveSnapshot::capture(&fixture.project).is_err(), "{path}");
    }
    let fixture = Fixture::new(SDL);
    fs::write(
        fixture.base.join("schema.graphql"),
        format!("{SDL}\nextend type Query {{ added: String }}"),
    )
    .unwrap();
    assert!(ActiveSnapshot::capture(&fixture.project).is_ok());
    let fixture = Fixture::new(SDL);
    fixture.write("generated/types.d.luau", "wrong");
    assert!(ActiveSnapshot::capture(&fixture.project).is_err());
    let fixture = Fixture::new(SDL);
    fs::create_dir(fixture.project.join(".chigiri-mock-transaction")).unwrap();
    assert!(ActiveSnapshot::capture(&fixture.project).is_err());
}

#[test]
fn ai_19_capture_releases_lock_and_detects_every_input_change_without_replacing_originals() {
    let _test_case = crate::vm_gate::test_case();
    for path in [
        "mock.luau",
        "generated/update-state.json",
        "generated/types.d.luau",
        "seeds/User.jsonl",
        "resolvers/query/getUsers.luau",
    ] {
        let fixture = Fixture::new(SDL);
        let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
        let original = snapshot.files()[0].source().to_owned();
        let directory = Directory::open(&fixture.project).unwrap();
        let lock = directory
            .lock(true)
            .expect("the concurrent edit must block update adoption");
        drop(lock);
        let bytes = fs::read_to_string(fixture.project.join(path)).unwrap();
        fixture.write(path, &(bytes + "\n"));
        assert!(snapshot.verify_unchanged().is_err(), "{path}");
        assert_eq!(snapshot.files()[0].source(), original);
    }
    let fixture = Fixture::new(SDL);
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    fixture.write("../schema.graphql", "invalid external SDL");
    assert!(snapshot.verify_unchanged().is_ok());
}

#[test]
fn ai_15_23_preflight_keeps_file_kind_and_aggregate_seed_limits() {
    let _test_case = crate::vm_gate::test_case();
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new(SDL);
    let path = fixture.project.join("seeds/User.jsonl");
    fs::remove_file(&path).unwrap();
    symlink(fixture.base.join("schema.graphql"), &path).unwrap();
    assert!(ActiveSnapshot::capture(&fixture.project).is_err());
    let fixture = Fixture::new(SDL);
    fixture.write("seeds/User.jsonl", &"x".repeat(crate::limits::SEED_BYTES));
    fixture.write("seeds/Post.jsonl", "x");
    assert!(ActiveSnapshot::capture(&fixture.project).is_err());
    let fixture = Fixture::new(SDL);
    fixture.write(
        "resolvers/query/getUsers.luau",
        &"x".repeat(crate::luau::MAX_BODY + 1),
    );
    assert!(ActiveSnapshot::capture(&fixture.project).is_err());
}
