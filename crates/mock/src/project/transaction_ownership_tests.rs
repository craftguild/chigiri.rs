//! Test hard-link collisions, no-follow parent and final ownership, and same-inode renames.

use std::fs;
use std::os::unix::fs::symlink;

use super::filesystem::TRANSACTION;
use super::test_support::Fixture;
use super::transaction::Point;
use super::transaction_test_support::{fixture, operations, tree};
use super::update::update_with;
use super::update_project;
use super::update_test_support::{DIST, MODULE, schema, state};

#[test]
fn mp_13_15_replacement_that_aliases_another_input_is_rejected_before_any_write() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(&schema("first"));
    fs::remove_file(fixture.project.join("seeds/Item.jsonl")).unwrap();
    fs::hard_link(
        fixture.project.join(MODULE),
        fixture.project.join("seeds/Item.jsonl"),
    )
    .unwrap();
    fs::write(fixture.base.join("schema.graphql"), schema("limit")).unwrap();
    let before = tree(&fixture.project);
    let error = update_project(&fixture.project).err().unwrap();
    assert_eq!(
        error.message,
        "replacement target shares another input identity"
    );
    assert_eq!(tree(&fixture.project), before);
    assert!(!fixture.project.join(TRANSACTION).exists());
}

#[test]
fn mp_11_15_adoption_allows_unchanged_user_and_dist_hardlinks() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = fixture();
    update_project(&fixture.project).unwrap();
    let next = fixture.read(DIST);
    fs::remove_file(fixture.project.join(MODULE)).unwrap();
    fs::hard_link(fixture.project.join(DIST), fixture.project.join(MODULE)).unwrap();
    let report = update_project(&fixture.project).unwrap();
    assert!(
        report
            .events
            .iter()
            .any(|entry| entry.path == MODULE && entry.action == super::UpdateAction::Adopted)
    );
    assert_eq!(fixture.read(MODULE), next);
    assert_eq!(fixture.read(DIST), next);
    let active = state(&fixture)
        .resolvers
        .0
        .into_iter()
        .find(|entry| entry.path == MODULE)
        .unwrap();
    assert_eq!(active.base, active.candidate);
    assert!(!fixture.project.join(TRANSACTION).exists());
}

#[test]
fn mp_13_15_symlink_fifo_and_replaced_parent_are_refused_before_writes() {
    let _test_case = crate::vm_gate::test_case();
    for path in [
        MODULE,
        "seeds/Item.jsonl",
        "generated/update-state.json",
        DIST,
    ] {
        for fifo in [false, true] {
            let fixture = fixture();
            let original = fixture.project.join(path);
            if original.exists() {
                fs::remove_file(&original).unwrap();
            }
            if fifo {
                assert!(
                    std::process::Command::new("mkfifo")
                        .arg(&original)
                        .status()
                        .unwrap()
                        .success()
                );
            } else {
                symlink(fixture.base.join("schema.graphql"), &original).unwrap();
            }
            assert!(update_project(&fixture.project).is_err(), "{path}");
            assert!(!fixture.project.join(TRANSACTION).exists());
            assert!(fs::symlink_metadata(original).is_ok());
        }
    }
    let fixture = fixture();
    let original = fixture.project.join("resolvers");
    let saved = fixture.base.join("saved-resolvers");
    fs::rename(&original, &saved).unwrap();
    symlink(&saved, &original).unwrap();
    assert!(update_project(&fixture.project).is_err());
    assert!(!fixture.project.join(TRANSACTION).exists());
    assert!(
        fs::symlink_metadata(original)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn mp_15_parent_replacement_after_preflight_preserves_both_directories_and_recovery() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = fixture();
    let mut paths = Vec::new();
    let before = tree(&fixture.project);
    let error = update_with(&fixture.project, |point| {
        if point == Point::Prepared {
            paths = operations(&fixture);
        }
        if let Point::BeforeInstall(index) = point
            && paths[index] == "resolvers/objects/Note/label.luau"
        {
            fs::rename(
                fixture.project.join("resolvers/objects/Note"),
                fixture.base.join("saved-note"),
            )
            .unwrap();
            fixture.write("resolvers/objects/Note/user-file", "preserve new parent");
        }
        Ok(())
    })
    .err()
    .unwrap();
    assert!(!error.committed);
    assert!(!error.recovery.is_empty());
    assert_eq!(
        fixture.read("resolvers/objects/Note/user-file"),
        "preserve new parent"
    );
    assert!(fixture.base.join("saved-note").is_dir());
    for path in [
        "generated/types.d.luau",
        "generated/schema.snapshot.json",
        "generated/update-state.json",
    ] {
        assert_eq!(fs::read(fixture.project.join(path)).unwrap(), before[path]);
    }
    assert!(
        fixture
            .project
            .join(TRANSACTION)
            .join("manifest.json")
            .is_file()
    );
}
