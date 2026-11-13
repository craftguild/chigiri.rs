//! Adversarial tests for descriptor-based verification and exclusive creation.

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Directory, FsError};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Fixture(pub(crate) PathBuf);

impl Fixture {
    pub(crate) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "chigiri-fs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    pub(super) fn directory(&self) -> Directory {
        Directory::open(&self.0).unwrap()
    }
    pub(super) fn fifo(&self) {
        // macOS lacks `mkfifoat`, so use the POSIX utility only to create the fixture.
        let output = crate::run_bounded_process(
            std::process::Command::new("mkfifo").arg(self.0.join("fifo")),
            &[],
            crate::ProcessLimits {
                timeout: std::time::Duration::from_secs(3),
                bytes: 1024,
            },
        )
        .unwrap();
        assert!(output.status.success());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sg_fs_02_rejects_parent_and_final_symlinks_and_fifos_with_open_descriptors() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("real")).unwrap();
    fs::write(fixture.0.join("real/input"), b"source").unwrap();
    symlink("real", fixture.0.join("link")).unwrap();
    symlink("real/input", fixture.0.join("input-link")).unwrap();
    assert!(Directory::open(&fixture.0.join("link")).is_err());
    let root = fixture.directory();
    assert!(root.read("link/input", 1024).is_err());
    assert!(root.read("input-link", 1024).is_err());
    fixture.fifo();
    assert_eq!(root.read("fifo", 1024), Err(FsError::Path));
}

#[test]
fn sg_fs_02_open_detects_path_replacement_behind_an_existing_descriptor() {
    let fixture = Fixture::new();
    let source = fixture.0.join("input");
    fs::write(&source, b"original").unwrap();
    let result = fixture
        .directory()
        .read_with_hook(OsStr::new("input"), 1024, || {
            fs::rename(&source, fixture.0.join("old")).unwrap();
            fs::write(&source, b"attacker").unwrap();
        });
    assert_eq!(result, Err(FsError::Changed));
    assert_eq!(fs::read(fixture.0.join("old")).unwrap(), b"original");
}

#[test]
fn sg_fs_02_read_detects_file_growth_and_syntax_changes() {
    let fixture = Fixture::new();
    let source = fixture.0.join("input");
    for (bytes, expected) in [
        (b"longer-content".as_slice(), FsError::Limit),
        (b"changed!".as_slice(), FsError::Changed),
    ] {
        fs::write(&source, b"original").unwrap();
        let result = fixture
            .directory()
            .read_with_hook(OsStr::new("input"), 8, || {
                fs::write(&source, bytes).unwrap();
            });
        assert_eq!(result, Err(expected));
    }
}

#[test]
fn sg_fs_02_rejects_operations_after_an_ancestor_is_replaced() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("parent/child")).unwrap();
    let root = Directory::open(&fixture.0.join("parent/child")).unwrap();
    fs::rename(fixture.0.join("parent"), fixture.0.join("retained")).unwrap();
    fs::create_dir_all(fixture.0.join("parent/child")).unwrap();
    assert_eq!(root.verify(), Err(FsError::Changed));
    assert_eq!(root.create("untouched", b"data", 10), Err(FsError::Changed));
    assert!(!fixture.0.join("parent/child/untouched").exists());
    assert!(!fixture.0.join("retained/child/untouched").exists());
}

#[test]
fn sg_cli_02_exclusive_creation_never_overwrites_files_directories_links_or_fifos() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    root.create("file", b"old", 100).unwrap();
    fs::create_dir(fixture.0.join("directory")).unwrap();
    symlink("absent", fixture.0.join("link")).unwrap();
    fixture.fifo();
    for name in ["file", "directory", "link", "fifo"] {
        assert_eq!(root.create(name, b"new", 100), Err(FsError::Exists));
    }
    assert_eq!(root.read("file", 100).unwrap(), b"old");
    assert!(fixture.0.join("directory").is_dir());
    assert_eq!(
        fs::read_link(fixture.0.join("link")).unwrap(),
        PathBuf::from("absent")
    );
}

#[test]
fn sg_fs_02_validates_relative_components_and_bytes_before_resource_limits() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    for path in [
        "../outside",
        "/absolute",
        "./file",
        "foo//bar",
        "foo/",
        "C:/file",
        "foo\\bar",
    ] {
        assert!(root.read(path, 1024).is_err());
    }
    assert_eq!(root.create("new", &[0; 9], 8), Err(FsError::Limit));
    assert!(!fixture.0.join("new").exists());
    fs::write(fixture.0.join("input"), [0; 9]).unwrap();
    assert_eq!(root.read("input", 8), Err(FsError::Limit));
    assert_eq!(root.read("input", 9).unwrap().len(), 9);
    assert!(root.read(&vec!["a"; 65].join("/"), 10).is_err());
}
