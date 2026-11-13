//! Verify exclusion and rename failures with real directories and existing entries.

use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;

use super::tests::Fixture;
use super::{Directory, FsError};

#[test]
fn sg_fs_05_lock_is_exclusive_across_clones_and_can_be_reacquired_after_drop() {
    let fixture = Fixture::new();
    let directory = fixture.directory();
    let lock = directory.lock().unwrap();
    assert!(matches!(directory.clone().lock(), Err(FsError::Busy)));
    assert!(matches!(fixture.directory().lock(), Err(FsError::Busy)));
    lock.verify().unwrap();
    drop(lock);
    fixture.directory().lock().unwrap();
}

#[test]
fn sg_fs_05_lock_excludes_a_competing_process_until_release() {
    let fixture = Fixture::new();
    let lock = fixture.directory().lock().unwrap();
    run_lock_child(&fixture, "busy");
    drop(lock);
    run_lock_child(&fixture, "exit");
    fixture.directory().lock().unwrap();
}

fn run_lock_child(fixture: &Fixture, mode: &str) {
    let output = crate::run_bounded_process(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "filesystem::mutation_tests::lock_child"])
            .env("CHIGIRI_TEST_LOCK_DIRECTORY", &fixture.0)
            .env("CHIGIRI_TEST_LOCK_MODE", mode),
        &[],
        crate::ProcessLimits {
            timeout: std::time::Duration::from_secs(3),
            bytes: 4096,
        },
    )
    .unwrap();
    assert!(output.status.success());
}

#[test]
fn lock_child() {
    let Some(path) = std::env::var_os("CHIGIRI_TEST_LOCK_DIRECTORY") else {
        return;
    };
    let directory = Directory::open(std::path::Path::new(&path)).unwrap();
    match std::env::var("CHIGIRI_TEST_LOCK_MODE").unwrap().as_str() {
        "busy" => assert!(matches!(directory.lock(), Err(FsError::Busy))),
        "exit" => {
            let _lock = directory.lock().unwrap();
            // If explicit completion fails, `Drop` lets the OS release the descriptor and lock.
            std::process::exit(0);
        }
        _ => panic!("fixture mode is undefined."),
    }
}

#[test]
fn sg_fs_02_lock_detects_parent_replacement_without_switching_trees() {
    let fixture = Fixture::new();
    let parent = fixture.directory().create_directory("parent").unwrap();
    let lock = parent.lock().unwrap();
    fs::rename(fixture.0.join("parent"), fixture.0.join("old")).unwrap();
    fs::create_dir(fixture.0.join("parent")).unwrap();
    assert_eq!(lock.verify(), Err(FsError::Changed));
    assert!(matches!(parent.lock(), Err(FsError::Changed)));
    assert!(matches!(
        parent.create_directory("x"),
        Err(FsError::Changed)
    ));
    assert!(!fixture.0.join("parent/x").exists());
    assert!(!fixture.0.join("old/x").exists());
}

#[test]
fn sg_fs_01_create_and_rename_does_not_overwrite_existing_entries_with_empty_dir() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    let source = root.create_directory("source").unwrap();
    source.create("kept", b"old", 100).unwrap();
    root.create_directory("directory").unwrap();
    root.create("file", b"original", 100).unwrap();
    symlink("missing", fixture.0.join("link")).unwrap();
    fixture.fifo();
    for name in ["directory", "file", "link", "fifo"] {
        assert_eq!(root.absent(name), Err(FsError::Exists));
        assert!(matches!(root.create_directory(name), Err(FsError::Exists)));
        let error = source.rename_to(&root, name).unwrap_err();
        assert!(error.moved.is_none());
        assert_eq!(source.read("kept", 100).unwrap(), b"old");
    }
    assert_eq!(root.read("file", 100).unwrap(), b"original");
    assert_eq!(
        fs::read_link(fixture.0.join("link")).unwrap(),
        std::path::Path::new("missing")
    );
}

#[test]
fn sg_fs_03_successful_rename_returns_a_new_handle_and_invalidates_the_old_handle() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    let source = root.create_directory("source").unwrap();
    source.create("kept", b"old", 100).unwrap();
    let target = root.create_directory("target").unwrap();
    let moved = source.rename_to(&target, "backup").unwrap();
    assert_eq!(source.verify(), Err(FsError::Changed));
    assert_eq!(moved.read("kept", 100).unwrap(), b"old");
    let expected = Directory::open(&fixture.0.join("target/backup")).unwrap();
    assert!(moved.same_location(&expected).unwrap());
    let restored = moved.rename_to(&root, "source").unwrap();
    assert_eq!(restored.read("kept", 100).unwrap(), b"old");
}

#[test]
fn sg_fs_03_post_rename_sync_failure_returns_moved_handle_for_restoration() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    let source = root.create_directory("source").unwrap();
    source.create("kept", b"old", 100).unwrap();
    let error = source
        .rename_with_hook(&root, "backup", || Err(FsError::Io))
        .unwrap_err();
    assert_eq!(error.error, FsError::Io);
    let moved = error.moved.unwrap();
    assert_eq!(moved.read("kept", 100).unwrap(), b"old");
    let restored = moved.rename_to(&root, "source").unwrap();
    assert_eq!(restored.read("kept", 100).unwrap(), b"old");
}

#[test]
fn sg_fs_02_parent_creation_rejects_links_and_empty_removal_preserves_nonempty_directories() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    let (parent, name) = root.ensure_parent("generated/nested/contract").unwrap();
    assert_eq!(name, "contract");
    root.ensure_parent("generated/nested/contract").unwrap();
    parent.create("kept", b"old", 100).unwrap();
    assert!(parent.remove_empty().is_err());
    assert_eq!(parent.read("kept", 100).unwrap(), b"old");
    symlink("generated", fixture.0.join("link")).unwrap();
    assert!(root.ensure_parent("link/new/contract").is_err());
    assert!(!fixture.0.join("generated/new").exists());
    root.create_directory("empty")
        .unwrap()
        .remove_empty()
        .unwrap();
    root.absent("empty").unwrap();
}

#[test]
fn sg_fs_04_empty_removal_preserves_a_substituted_directory() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    let expected = root.create_directory("raced").unwrap();
    assert_eq!(
        expected.remove_empty_with_hook(|| {
            fs::rename(fixture.0.join("raced"), fixture.0.join("retained")).unwrap();
            fs::create_dir(fixture.0.join("raced")).unwrap();
            fs::write(fixture.0.join("raced/attacker"), b"keep").unwrap();
        }),
        Err(FsError::Changed)
    );
    assert!(fixture.0.join("retained").is_dir());
    assert_eq!(root.read("raced/attacker", 100).unwrap(), b"keep");
}
