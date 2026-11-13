//! Treats the complete configuration boundary as one rollback unit without
//! requiring readiness after a successful update.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::filesystem::TRANSACTION;
use super::transaction::Point;
use super::transaction_test_support::{fixture, injected, operations, tree};
use super::update::update_with;
use super::update_test_support::{DIST, MODULE, state};
use super::{read_project, update_project};

#[test]
fn mp_09_14_16_real_update_preserves_edits_and_seed_without_readiness_and_is_idempotent() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = fixture();
    let before = tree(&fixture.project);
    let old = state(&fixture);
    let report = update_with(&fixture.project, |point| {
        if point == Point::Prepared {
            let stage = fixture.project.join(TRANSACTION);
            assert_eq!(
                fs::metadata(&stage).unwrap().permissions().mode() & 0o777,
                0o700
            );
            for entry in fs::read_dir(stage).unwrap() {
                assert_eq!(
                    entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            assert!(operations(&fixture).iter().any(|path| path == DIST));
        }
        Ok(())
    })
    .unwrap();
    assert!(report.schema_changed);
    for path in ["mock.luau", MODULE, "seeds/Item.jsonl"] {
        assert_eq!(fs::read(fixture.project.join(path)).unwrap(), before[path]);
    }
    assert!(fixture.read(DIST).contains("Signature:"));
    assert!(fixture.read("seeds/Note.jsonl").is_empty());
    assert!(!fixture.project.join(TRANSACTION).exists());
    assert_ne!(state(&fixture).generation, old.generation);
    let error = read_project(&fixture.project).err().unwrap();
    assert_eq!(error.file, "seeds/Item.jsonl");
    let placed = tree(&fixture.project);
    let generation = state(&fixture).generation;
    assert!(!update_project(&fixture.project).unwrap().schema_changed);
    assert_eq!(tree(&fixture.project), placed);
    assert_eq!(state(&fixture).generation, generation);
}

#[test]
fn mp_15_each_candidate_backup_install_and_final_validation_failure_restores_entire_unit() {
    let _test_case = crate::vm_gate::test_case();
    let probe = fixture();
    let mut points = Vec::new();
    update_with(&probe.project, |point| {
        if !matches!(
            point,
            Point::Committed | Point::BeforeCleanup(_) | Point::AfterCleanup(_)
        ) {
            points.push(point);
        }
        Ok(())
    })
    .unwrap();
    assert!(
        points
            .iter()
            .any(|point| matches!(point, Point::AfterBackup(_)))
    );
    assert!(
        points
            .iter()
            .any(|point| matches!(point, Point::AfterInstall(_)))
    );
    for target in points {
        let fixture = fixture();
        let before = tree(&fixture.project);
        let mut triggered = false;
        let result = update_with(&fixture.project, |point| {
            if point == target {
                triggered = true;
                return Err(injected());
            }
            Ok(())
        });
        let error = result.err().expect("fault must reject placement");
        assert!(triggered, "{target:?}");
        assert!(!error.committed, "{target:?}: {error}");
        assert!(error.recovery.is_empty(), "{target:?}: {error}");
        assert_eq!(tree(&fixture.project), before, "{target:?}");
        assert!(!fixture.project.join(TRANSACTION).exists(), "{target:?}");
    }
}

#[test]
fn mp_15_each_committed_cleanup_failure_keeps_new_generation_and_recovery_marker() {
    let _test_case = crate::vm_gate::test_case();
    let probe = fixture();
    let mut points = Vec::new();
    update_with(&probe.project, |point| {
        if matches!(
            point,
            Point::Committed | Point::BeforeCleanup(_) | Point::AfterCleanup(_)
        ) {
            points.push(point);
        }
        Ok(())
    })
    .unwrap();
    for target in points {
        let fixture = fixture();
        let mut committed = None;
        let error = update_with(&fixture.project, |point| {
            if point == Point::Committed {
                committed = Some(tree(&fixture.project));
            }
            if point == target {
                return Err(injected());
            }
            Ok(())
        })
        .err()
        .unwrap();
        assert!(error.committed, "{target:?}: {error}");
        assert_eq!(tree(&fixture.project), committed.unwrap(), "{target:?}");
        assert!(fixture.project.join(TRANSACTION).is_dir());
        assert_eq!(
            super::transaction_test_support::recovery(&fixture)["committed"],
            true
        );
        assert!(read_project(&fixture.project).is_err());
        assert!(update_project(&fixture.project).is_err());
        assert!(
            super::export_project(&fixture.project, &fixture.base.join("bundle.toml")).is_err()
        );
        assert!(!fixture.base.join("bundle.toml").exists());
    }
}
