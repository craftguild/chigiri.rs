//! Verify recovery rejects missing or modified backups, resumes after one failure, and preserves evidence.

use std::collections::BTreeSet;
use std::fs;

use super::filesystem::TRANSACTION;
use super::transaction::Point;
use super::transaction_test_support::{fixture, injected, operations, recovery, tree};
use super::update::update_with;

#[test]
fn mp_15_missing_and_modified_backup_preserve_primary_and_attempt_every_restore() {
    let _test_case = crate::vm_gate::test_case();
    for remove in [true, false] {
        let fixture = fixture();
        let before = tree(&fixture.project);
        let mut paths = Vec::new();
        let mut attempted = BTreeSet::new();
        let error = update_with(&fixture.project, |point| {
            match point {
                Point::Prepared => paths = operations(&fixture),
                Point::BeforeCommit => {
                    let backup = fixture.project.join(TRANSACTION).join("backup-1");
                    if remove {
                        fs::remove_file(backup).unwrap();
                    } else {
                        fs::write(backup, b"edited recovery material").unwrap();
                    }
                    return Err(injected());
                }
                Point::BeforeRestore(index) => {
                    attempted.insert(index);
                }
                _ => {}
            }
            Ok(())
        })
        .err()
        .unwrap();
        assert_eq!(error.message, injected().message);
        assert!(!error.committed);
        assert!(!error.recovery.is_empty());
        assert_eq!(attempted, (0..paths.len()).collect());
        for (index, path) in paths.iter().enumerate() {
            if index == 1 {
                continue;
            }
            assert_eq!(
                fs::read(fixture.project.join(path)).ok(),
                before.get(path).cloned(),
                "{path}"
            );
        }
        assert!(!fixture.project.join(&paths[1]).exists());
        assert!(
            fixture
                .project
                .join(TRANSACTION)
                .join("candidate-1")
                .is_file()
        );
        if !remove {
            assert_eq!(
                fixture.read(&format!("{TRANSACTION}/backup-1")),
                "edited recovery material"
            );
        }
        let record = recovery(&fixture);
        assert_eq!(record["committed"], false);
        assert_eq!(record["primary"]["message"], injected().message);
        assert!(!record["recovery"].as_array().unwrap().is_empty());
        assert_eq!(record["operations"].as_array().unwrap().len(), paths.len());
    }
}

#[test]
fn mp_15_restore_io_failure_does_not_skip_later_targets_or_delete_backup() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = fixture();
    let before = tree(&fixture.project);
    let mut paths = Vec::new();
    let mut attempted = Vec::new();
    let error = update_with(&fixture.project, |point| {
        match point {
            Point::Prepared => paths = operations(&fixture),
            Point::BeforeCommit => return Err(injected()),
            Point::BeforeRestore(index) => {
                attempted.push(index);
                if index == 1 {
                    return Err(injected());
                }
            }
            _ => {}
        }
        Ok(())
    })
    .err()
    .unwrap();
    assert_eq!(attempted, (0..paths.len()).rev().collect::<Vec<_>>());
    assert_eq!(error.recovery.len(), 1, "{error}");
    assert_eq!(error.recovery[0].file.as_str(), paths[1]);
    assert_ne!(
        fs::read(fixture.project.join(&paths[1])).unwrap(),
        before[&paths[1]]
    );
    assert_eq!(
        fs::read(fixture.project.join(TRANSACTION).join("backup-1")).unwrap(),
        before[&paths[1]]
    );
    for (index, path) in paths.iter().enumerate() {
        if index != 1 {
            assert_eq!(
                fs::read(fixture.project.join(path)).ok(),
                before.get(path).cloned()
            );
        }
    }
    assert_eq!(recovery(&fixture)["operations"][1]["installed"], true);
}

#[test]
fn mp_15_edits_to_installed_or_restored_files_are_preserved_and_reported() {
    let _test_case = crate::vm_gate::test_case();
    for after in [false, true] {
        let fixture = fixture();
        let before = tree(&fixture.project);
        let mut paths = Vec::new();
        let error = update_with(&fixture.project, |point| {
            match point {
                Point::Prepared => paths = operations(&fixture),
                Point::BeforeCommit => return Err(injected()),
                Point::BeforeRestore(1) if !after => {
                    fixture.write(&paths[1], "concurrent user edit")
                }
                Point::AfterRestore(1) if after => fixture.write(&paths[1], "concurrent user edit"),
                _ => {}
            }
            Ok(())
        })
        .err()
        .unwrap();
        assert!(!error.recovery.is_empty());
        assert_eq!(fixture.read(&paths[1]), "concurrent user edit");
        assert_eq!(
            fs::read(fixture.project.join(&paths[0])).unwrap(),
            before[&paths[0]]
        );
        assert!(
            fixture
                .project
                .join(TRANSACTION)
                .join("manifest.json")
                .exists()
        );
        assert!(
            !recovery(&fixture)["recovery"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        if !after {
            assert_eq!(
                fs::read(fixture.project.join(TRANSACTION).join("backup-1")).unwrap(),
                before[&paths[1]]
            );
        }
    }
}

#[test]
fn mp_15_unknown_or_edited_stage_files_are_never_recursively_cleaned() {
    let _test_case = crate::vm_gate::test_case();
    for (point, path) in [
        (Point::Prepared, "user-note"),
        (Point::Committed, "user-note"),
        (Point::CandidateCreated(0), "candidate-0"),
    ] {
        let fixture = fixture();
        let error = update_with(&fixture.project, |at| {
            if at == point {
                fixture.write(&format!("{TRANSACTION}/{path}"), "preserve this file");
            }
            Ok(())
        })
        .err()
        .unwrap();
        assert_eq!(error.committed, point == Point::Committed);
        assert_eq!(
            fixture.read(&format!("{TRANSACTION}/{path}")),
            "preserve this file"
        );
        assert!(
            fixture
                .project
                .join(TRANSACTION)
                .join("manifest.json")
                .exists()
        );
        assert_eq!(recovery(&fixture)["committed"], error.committed);
    }
}
