//! Verifies that every post-commit cleanup failure keeps the new live tree and
//! a complete recovery copy of the old bytes.

use crate::filesystem::{CapturedTree, FsError};

use super::Operation;
use super::tests::{live, prepared};
use super::transaction::{STAGE, Step, Transaction, capture};

#[test]
fn failed_restore_keeps_the_new_live_tree_and_complete_old_backup() {
    let (fixture, mut transaction, old) = prepared();
    let error = transaction
        .install(&fixture.manifest, &mut |step| {
            if step == Step::RecoveryWrite {
                Err(FsError::Io)
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    let error = transaction.finish_failure(error);
    assert!(error.to_string().contains("already live"));
    assert!(transaction.committed);
    assert_eq!(Some(&live(&fixture)), transaction.expected.as_ref());
    assert_eq!(
        capture(&transaction.stage.relative("previous").unwrap()).unwrap(),
        old
    );
}

#[test]
fn sg_fs_04_archive_preserves_all_old_bytes_after_partial_backup_cleanup() {
    let (fixture, mut transaction, old) = prepared();
    let mut entries = 0;
    let error = transaction
        .install(&fixture.manifest, &mut |step| {
            if step == Step::CleanupEntry {
                entries += 1;
                if entries == 2 {
                    return Err(FsError::Io);
                }
            }
            Ok(())
        })
        .unwrap_err();
    let error = transaction.finish_failure(error);
    assert!(error.to_string().contains("already live"));
    assert_eq!(Some(&live(&fixture)), transaction.expected.as_ref());
    assert_eq!(archive(&transaction), old);
    let partial = capture(&transaction.stage.relative("previous").unwrap()).unwrap();
    assert_eq!(partial.files.len() + 1, old.files.len());
}

#[test]
fn failed_cleanup_prevents_a_subsequent_run() {
    for failed in [Step::StageRemove, Step::RecoveryRemove] {
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
        let error = transaction.finish_failure(error);
        assert!(error.to_string().contains("already live"));
        assert_eq!(Some(&live(&fixture)), transaction.expected.as_ref());
        assert_eq!(archive(&transaction), old);
        assert_eq!(
            transaction.parent.absent(STAGE).is_err(),
            failed == Step::StageRemove
        );
        drop(transaction);
        assert!(Transaction::begin(&fixture.manifest, Operation::Update).is_err());
    }
}

#[test]
fn successful_cleanup_keeps_new_live_tree_and_removes_restore_artifacts() {
    let (fixture, mut transaction, old) = prepared();
    transaction
        .install(&fixture.manifest, &mut |_| Ok(()))
        .unwrap();
    assert_ne!(live(&fixture), old);
    assert_eq!(Some(&live(&fixture)), transaction.expected.as_ref());
    assert_eq!(transaction.parent.absent(STAGE), Ok(()));
    assert_eq!(transaction.parent.absent(super::recovery::NAME), Ok(()));
}

#[test]
fn sg_fs_04_extractor_restores_old_bytes_and_rejects_existing_or_corrupt_archives() {
    let (fixture, mut transaction, old) = prepared();
    transaction
        .install(&fixture.manifest, &mut |step| {
            if step == Step::RecoveryRemove {
                Err(FsError::Io)
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    let destination = fixture.root.join("recovered");
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/recover-generated-crate.py");
    let recovery = crate::filesystem::PortableRelativePath::new(super::recovery::NAME).unwrap();
    let run = |source: Option<&std::path::Path>, destination: &std::path::Path| {
        let mut command = std::process::Command::new("python3");
        command.arg(&script);
        if let Some(source) = source {
            command.arg(source);
        } else {
            transaction
                .parent
                .push_descendant_argument(&mut command, &recovery)
                .unwrap();
        }
        command.arg(destination);
        crate::run_bounded_process(
            &mut command,
            &[],
            crate::ProcessLimits {
                timeout: std::time::Duration::from_secs(5),
                bytes: 8192,
            },
        )
        .unwrap()
    };
    let restored = run(None, &destination);
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    assert_eq!(
        capture(&crate::filesystem::Directory::open(&destination).unwrap()).unwrap(),
        old
    );
    assert!(!run(None, &destination).status.success());
    assert_eq!(
        capture(&crate::filesystem::Directory::open(&destination).unwrap()).unwrap(),
        old
    );
    let mut bytes = transaction
        .parent
        .read(super::recovery::NAME, 64 * 1024 * 1024)
        .unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    let broken = fixture.root.join("broken");
    std::fs::write(&broken, bytes).unwrap();
    let rejected = fixture.root.join("rejected");
    assert!(!run(Some(&broken), &rejected).status.success());
    assert!(!rejected.exists());
}

pub(super) fn archive(transaction: &Transaction) -> CapturedTree {
    let bytes = transaction
        .parent
        .read(super::recovery::NAME, 64 * 1024 * 1024)
        .unwrap();
    let mut cursor = super::recovery::MAGIC.len();
    assert_eq!(&bytes[..cursor], super::recovery::MAGIC);
    let name_len = take_u32(&bytes, &mut cursor) as usize;
    assert_eq!(
        &bytes[cursor..cursor + name_len],
        transaction.name.as_bytes()
    );
    cursor += name_len;
    let count = take_u64(&bytes, &mut cursor);
    let mut files = std::collections::BTreeMap::new();
    for _ in 0..count {
        let name_len = take_u32(&bytes, &mut cursor) as usize;
        let length = take_u64(&bytes, &mut cursor) as usize;
        let path = std::str::from_utf8(&bytes[cursor..cursor + name_len])
            .unwrap()
            .to_owned();
        cursor += name_len;
        files.insert(path, bytes[cursor..cursor + length].to_vec());
        cursor += length;
    }
    assert_eq!(cursor, bytes.len());
    CapturedTree::try_from_files(files).unwrap()
}

fn take_u32(bytes: &[u8], cursor: &mut usize) -> u32 {
    let value = u32::from_be_bytes(bytes[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    value
}

fn take_u64(bytes: &[u8], cursor: &mut usize) -> u64 {
    let value = u64::from_be_bytes(bytes[*cursor..*cursor + 8].try_into().unwrap());
    *cursor += 8;
    value
}
