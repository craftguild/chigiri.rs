//! Reject user edits or late file creation after observation begins, leaving those files untouched.

use std::fs;

use super::filesystem::TRANSACTION;
use super::transaction::Point;
use super::transaction_test_support::{fixture, operations, tree};
use super::update::update_with;
use super::update_test_support::MODULE;

#[test]
fn mp_15_late_new_file_and_parent_creation_preserve_user_data_without_partial_placement() {
    let _test_case = crate::vm_gate::test_case();
    for new_parent in [true, false] {
        let fixture = fixture();
        let mut expected = tree(&fixture.project);
        let mut paths = Vec::new();
        let mut injected = false;
        let error = update_with(&fixture.project, |point| {
            if point == Point::Prepared {
                paths = operations(&fixture);
                if new_parent {
                    fixture.write("resolvers/objects/Note/user-note", "preserve");
                    expected = tree(&fixture.project);
                    injected = true;
                }
            }
            if let Point::BeforeInstall(index) = point
                && !new_parent
                && paths[index] == "seeds/Note.jsonl"
            {
                fixture.write(&paths[index], "late seed");
                expected.insert(paths[index].clone(), b"late seed".to_vec());
                injected = true;
            }
            Ok(())
        })
        .err()
        .unwrap();
        assert!(injected);
        assert!(error.recovery.is_empty(), "{error}");
        assert_eq!(tree(&fixture.project), expected);
        assert!(!fixture.project.join(TRANSACTION).exists());
    }
}

#[test]
fn mp_15_concurrent_kept_module_seed_config_and_sdl_edits_abort_at_end_snapshot() {
    let _test_case = crate::vm_gate::test_case();
    for path in [MODULE, "seeds/Item.jsonl", "mock.luau", "../schema.graphql"] {
        let fixture = fixture();
        let mut expected = tree(&fixture.project);
        let error = update_with(&fixture.project, |point| {
            if point == Point::BeforeCommit {
                fs::write(fixture.project.join(path), b"concurrent edit").unwrap();
                if !path.starts_with("../") {
                    expected.insert(path.into(), b"concurrent edit".to_vec());
                }
            }
            Ok(())
        })
        .err()
        .unwrap();
        assert!(!error.committed);
        assert!(error.recovery.is_empty(), "{path}: {error}");
        assert_eq!(tree(&fixture.project), expected);
        assert_eq!(
            fs::read(fixture.project.join(path)).unwrap(),
            b"concurrent edit"
        );
        assert!(!fixture.project.join(TRANSACTION).exists());
    }
}

#[test]
fn mp_15_late_existing_destination_is_preserved_with_original_backup_for_recovery() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = fixture();
    let mut paths = Vec::new();
    let before = tree(&fixture.project);
    let error = update_with(&fixture.project, |point| {
        if point == Point::Prepared {
            paths = operations(&fixture);
        }
        if point == Point::AfterBackup(1) {
            fixture.write(&paths[1], "late destination");
        }
        Ok(())
    })
    .err()
    .unwrap();
    assert!(!error.committed);
    assert!(!error.recovery.is_empty());
    assert_eq!(fixture.read(&paths[1]), "late destination");
    assert_eq!(
        fs::read(fixture.project.join(TRANSACTION).join("backup-1")).unwrap(),
        before[&paths[1]]
    );
    assert_eq!(
        fs::read(fixture.project.join(&paths[0])).unwrap(),
        before[&paths[0]]
    );
}
