//! Verify rename fallback restores moved handles without losing known candidates or backups.

use std::ffi::OsStr;
use std::fs;

use super::filesystem::Directory;
use super::transaction_test_support::{fixture, injected};

#[test]
fn mp_15_file_post_rename_failure_retains_moved_handle_for_rollback() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = fixture();
    let root = Directory::open(&fixture.project).unwrap();
    let original = root
        .create_file(OsStr::new("original"), "original bytes")
        .unwrap();
    let failed = original
        .move_with(&root, OsStr::new("moved"), || Err(injected()))
        .err()
        .unwrap();
    assert_eq!(failed.error.message, injected().message);
    let moved = failed.moved.unwrap();
    assert!(moved.verify().is_ok());
    assert!(!fixture.project.join("original").exists());
    assert_eq!(fixture.read("moved"), "original bytes");
    assert!(moved.move_to(&root, OsStr::new("original")).is_ok());
    assert_eq!(fixture.read("original"), "original bytes");
}

#[test]
fn mp_15_changed_post_rename_target_remains_available_but_is_not_trusted_for_restore() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = fixture();
    let root = Directory::open(&fixture.project).unwrap();
    let original = root
        .create_file(OsStr::new("original"), "original bytes")
        .unwrap();
    let failed = original
        .move_with(&root, OsStr::new("moved"), || {
            fixture.write("moved", "changed bytes");
            Ok(())
        })
        .err()
        .unwrap();
    let moved = failed.moved.unwrap();
    assert!(moved.verify().is_err());
    let restore = moved.move_to(&root, OsStr::new("original")).err().unwrap();
    assert!(restore.moved.is_none());
    assert_eq!(fixture.read("moved"), "changed bytes");
    assert!(!fixture.project.join("original").exists());
}

#[test]
fn mp_15_directory_post_rename_failure_retains_identity_and_private_contents() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = fixture();
    let root = Directory::open(&fixture.project).unwrap();
    let original = root.create_directory(OsStr::new("original")).unwrap();
    original
        .create_file(OsStr::new("data"), "preserve")
        .unwrap();
    let failed = original
        .move_with(&root, OsStr::new("moved"), || Err(injected()))
        .err()
        .unwrap();
    let moved = failed.moved.unwrap();
    assert_eq!(moved.identity(), original.identity());
    assert!(moved.verify().is_ok());
    assert!(!fixture.project.join("original").exists());
    assert_eq!(fixture.read("moved/data"), "preserve");
    assert!(moved.move_to(&root, OsStr::new("original")).is_ok());
    assert_eq!(
        fs::read(fixture.project.join("original/data")).unwrap(),
        b"preserve"
    );
}
