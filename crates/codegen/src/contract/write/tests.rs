//! Injects rename-boundary and real-Cargo failures while checking live bytes around transactions.

use std::fs;

use crate::contract::check::fixture::Fixture;
use crate::filesystem::{CapturedTree, Directory, FsError};
use crate::{check_contract, generate_contract, render_contract, update_contract};

use super::Operation;
use super::transaction::{STAGE, Step, Transaction, capture};

pub(super) fn prepared() -> (Fixture, Transaction, CapturedTree) {
    let fixture = Fixture::new();
    fixture.write(&render_contract(&fixture.manifest).unwrap());
    let old = capture(&Directory::open(&fixture.output()).unwrap()).unwrap();
    fs::write(
        fixture.root.join("graphql/schema.graphql"),
        "type Query { added: Int value: String }",
    )
    .unwrap();
    let mut transaction = Transaction::begin(&fixture.manifest, Operation::Update).unwrap();
    transaction
        .prepare(render_contract(&fixture.manifest).unwrap())
        .unwrap();
    (fixture, transaction, old)
}

pub(super) fn live(fixture: &Fixture) -> CapturedTree {
    capture(&Directory::open(&fixture.output()).unwrap()).unwrap()
}

#[test]
fn sg_fs_01_generate_is_absent_so_update_is_required() {
    let fixture = Fixture::new();
    for source in [
        None,
        Some("old module"),
        Some("{}"),
        Some("{\"schema_version\":\"legacy\"}"),
    ] {
        fs::create_dir_all(fixture.output()).unwrap();
        if let Some(source) = source {
            fs::write(fixture.output().join(crate::GENERATED_MARKER_FILE), source).unwrap();
        }
        assert!(Transaction::begin(&fixture.manifest, Operation::Generate).is_err());
        assert!(Transaction::begin(&fixture.manifest, Operation::Update).is_err());
        fs::remove_dir_all(fixture.output()).unwrap();
    }
    fixture.write(&render_contract(&fixture.manifest).unwrap());
    fs::write(fixture.output().join("handwritten.rs"), b"kept").unwrap();
    assert!(Transaction::begin(&fixture.manifest, Operation::Update).is_err());
    assert_eq!(
        fs::read(fixture.output().join("handwritten.rs")).unwrap(),
        b"kept"
    );
}

#[test]
fn sg_fs_03_precommit_failures_restore_the_previous_output_at_every_install_step() {
    for failed in [Step::Backup, Step::Install, Step::Verify] {
        let (fixture, mut transaction, old) = prepared();
        let error = transaction
            .install(&fixture.manifest, &mut |step| {
                if step == failed {
                    Err(FsError::Io)
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        let _ = transaction.finish_failure(error);
        assert_eq!(live(&fixture), old, "{failed:?}");
        assert_eq!(transaction.parent.absent(STAGE), Ok(()), "{failed:?}");
        assert_eq!(transaction.parent.absent(super::recovery::NAME), Ok(()));
    }
}

#[test]
fn sg_fs_03_failed_initial_verification_removes_the_new_output() {
    let fixture = Fixture::new();
    let mut transaction = Transaction::begin(&fixture.manifest, Operation::Generate).unwrap();
    transaction
        .prepare(render_contract(&fixture.manifest).unwrap())
        .unwrap();
    let error = transaction
        .install(&fixture.manifest, &mut |step| {
            if step == Step::Verify {
                Err(FsError::Io)
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    let _ = transaction.finish_failure(error);
    assert!(!fixture.output().exists());
    assert_eq!(transaction.parent.absent(STAGE), Ok(()));
}

#[test]
fn failed_install_and_rollback_preserve_old_backup_and_both_causes() {
    let (fixture, mut transaction, old) = prepared();
    let error = transaction
        .install(&fixture.manifest, &mut |step| {
            if matches!(step, Step::Install | Step::Rollback) {
                Err(FsError::Io)
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    let error = transaction.finish_failure(error);
    assert!(error.to_string().contains("rollback"));
    assert!(error.report().diagnostics().len() >= 3);
    assert!(!fixture.output().exists());
    assert_eq!(
        capture(&transaction.stage.relative("previous").unwrap()).unwrap(),
        old
    );
    drop(transaction);
    assert!(Transaction::begin(&fixture.manifest, Operation::Update).is_err());
}

#[test]
fn failed_candidate_install_preserves_both_live_and_backup_trees() {
    let (fixture, mut transaction, old) = prepared();
    let error = transaction
        .install(&fixture.manifest, &mut |step| {
            if matches!(step, Step::Verify | Step::Uninstall) {
                Err(FsError::Io)
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    let error = transaction.finish_failure(error);
    assert!(error.to_string().contains("rollback"));
    assert_eq!(Some(&live(&fixture)), transaction.expected.as_ref());
    assert_eq!(
        capture(&transaction.stage.relative("previous").unwrap()).unwrap(),
        old
    );
}

#[test]
fn sg_fs_05_concurrent_output_rejects_stage_without_modifying_unrelated_files() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("unrelated"), b"untouched").unwrap();
    let transaction = Transaction::begin(&fixture.manifest, Operation::Generate).unwrap();
    assert!(Transaction::begin(&fixture.manifest, Operation::Generate).is_err());
    drop(transaction);
    assert!(Transaction::begin(&fixture.manifest, Operation::Generate).is_err());
    assert_eq!(
        fs::read(fixture.root.join("unrelated")).unwrap(),
        b"untouched"
    );
}

#[test]
fn sg_proc_03_real_cargo_checks_isolated_candidates_and_reuses_the_external_cache() {
    if isolated_cargo_config_test(
        "contract::write::tests::sg_proc_03_real_cargo_checks_isolated_candidates_and_reuses_the_external_cache",
    ) {
        return;
    }
    let fixture = Fixture::new();
    let cache = configure_cache(&fixture);
    generate_contract(&fixture.manifest, crate::DiagnosticPolicy::default()).unwrap();
    check_contract(&fixture.manifest).unwrap();
    let old = live(&fixture);
    fs::write(
        fixture.root.join("graphql/schema.graphql"),
        "type Query { added: Int value: String }",
    )
    .unwrap();
    update_contract(&fixture.manifest, crate::DiagnosticPolicy::default()).unwrap();
    check_contract(&fixture.manifest).unwrap();
    assert_ne!(live(&fixture), old);
    assert!(cache.is_dir());
    for path in ["Cargo.lock", "target", ".cargo"] {
        assert!(!fixture.output().join(path).exists());
    }
    let parent = fixture.output().parent().unwrap().to_owned();
    assert!(!parent.join(STAGE).exists());
    assert!(!parent.join(super::recovery::NAME).exists());
}

pub(super) fn isolated_cargo_config_test(name: &str) -> bool {
    const CHILD: &str = "CHIGIRI_CARGO_CONFIG_TEST";
    if std::env::var_os(CHILD).as_deref() == Some(std::ffi::OsStr::new(name)) {
        return false;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name])
        .env(CHILD, name)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("CARGO_BUILD_BUILD_DIR")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{name} failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed; 0 ignored"),
        "the isolated test must execute exactly one non-ignored case",
    );
    true
}

pub(super) fn configure_cache(fixture: &Fixture) -> std::path::PathBuf {
    let repository =
        fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let cache = repository.join("target/sibling-generator/contract-cache");
    fs::create_dir(fixture.root.join(".cargo")).unwrap();
    fs::write(
        fixture.root.join(".cargo/config.toml"),
        format!(
            "[build]\ntarget-dir={}\n[net]\noffline=true\n",
            toml_edit::Value::from(cache.to_str().unwrap())
        ),
    )
    .unwrap();
    cache
}
