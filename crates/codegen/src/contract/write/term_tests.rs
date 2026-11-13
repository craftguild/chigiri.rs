//! Sends TERM at each boundary before and after commit and verifies rollback or
//! recovery of the old bytes.

mod process;

use super::tests::{live, prepared};
use super::transaction::{STAGE, Step, capture};
use crate::filesystem::FsError;

#[test]
fn sg_fs_05_interrupted_commit_restores_the_previous_live_bytes() {
    for phase in [
        "backup", "install", "verify", "recovery", "cleanup", "stage", "archive",
    ] {
        let output = crate::run_bounded_process(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "contract::write::term_tests::phase_child"])
                .env("CHIGIRI_TEST_TERM_PHASE", phase),
            &[],
            crate::ProcessLimits {
                timeout: std::time::Duration::from_secs(30),
                bytes: 8192,
            },
        )
        .unwrap();
        assert!(
            output.status.success(),
            "{phase}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn phase_child() {
    let Ok(phase) = std::env::var("CHIGIRI_TEST_TERM_PHASE") else {
        return;
    };
    let (fixture, mut transaction, old) = prepared();
    let selected = match phase.as_str() {
        "backup" => Step::Backup,
        "install" => Step::Install,
        "verify" => Step::Verify,
        "recovery" => Step::RecoveryWrite,
        "cleanup" => Step::CleanupEntry,
        "stage" => Step::StageRemove,
        "archive" => Step::RecoveryRemove,
        _ => panic!("The undefined TERM fixture"),
    };
    let mut cleanup_entries = 0;
    let error = crate::with_signal_cancellation(|| {
        let primary = transaction
            .install(&fixture.manifest, &mut |step| {
                if step == Step::CleanupEntry {
                    cleanup_entries += 1;
                }
                if (step == selected && (step != Step::CleanupEntry || cleanup_entries == 2))
                    || step == Step::Rollback
                {
                    // An extra TERM signal must not interrupt rollback.
                    signal_hook::low_level::raise(signal_hook::consts::SIGTERM).unwrap();
                }
                Ok(())
            })
            .unwrap_err();
        transaction.finish_failure(primary)
    })
    .unwrap();
    assert!(
        error
            .report()
            .diagnostics()
            .iter()
            .any(|item| item.code() == "SF4026")
    );
    match selected {
        Step::Backup | Step::Install | Step::Verify => {
            assert!(!transaction.committed);
            assert_eq!(live(&fixture), old);
            assert_eq!(transaction.parent.absent(STAGE), Ok(()));
        }
        Step::RecoveryWrite => {
            assert!(transaction.committed);
            assert_eq!(Some(&live(&fixture)), transaction.expected.as_ref());
            assert_eq!(
                capture(&transaction.stage.relative("previous").unwrap()).unwrap(),
                old
            );
            assert!(error.to_string().contains("already live"));
        }
        _ => {
            assert!(transaction.committed);
            assert_eq!(Some(&live(&fixture)), transaction.expected.as_ref());
            assert_eq!(super::cleanup_tests::archive(&transaction), old);
            assert!(error.to_string().contains("already live"));
            if selected == Step::CleanupEntry {
                let partial = capture(&transaction.stage.relative("previous").unwrap()).unwrap();
                assert_eq!(partial.files.len() + 1, old.files.len());
            }
        }
    }
}

#[test]
fn sg_fs_03_hold_both_cancellation_and_rollback_failure() {
    let (fixture, mut transaction, old) = prepared();
    let token = crate::Cancellation::default();
    let error = token.run(|| {
        let error = transaction
            .install(&fixture.manifest, &mut |step| {
                if step == Step::Install {
                    token.cancel();
                }
                if step == Step::Rollback {
                    Err(FsError::Io)
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        transaction.finish_failure(error)
    });
    assert!(error.to_string().contains("SF4026"));
    assert!(error.to_string().contains("rollback"));
    assert!(!fixture.output().exists());
    assert_eq!(
        capture(&transaction.stage.relative("previous").unwrap()).unwrap(),
        old
    );
}
