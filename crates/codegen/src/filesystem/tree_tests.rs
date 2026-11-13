//! Verifies tree-capture resource limits and path changes against a real filesystem.

use std::fs;
use std::os::unix::fs::symlink;

use super::tests::Fixture;
use super::{FsError, TreeLimits};

fn limits() -> TreeLimits {
    TreeLimits {
        depth: 64,
        entries: 100,
        files: 100,
        file_bytes: 1024,
        total_bytes: 4096,
    }
}

#[test]
fn sg_fs_02_tree_captures_inventory_and_path_sequence_bytes_with_empty_directory() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("nested/empty")).unwrap();
    fs::write(fixture.0.join("z.graphql"), b"last").unwrap();
    fs::write(fixture.0.join("nested/a.gql"), b"first").unwrap();
    let tree = fixture.directory().capture(limits(), |_| true).unwrap();
    assert_eq!(
        tree.files
            .keys()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        ["nested/a.gql", "z.graphql"]
    );
    assert_eq!(tree.files["nested/a.gql"], b"first");
    assert_eq!(
        tree.directories
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        ["nested", "nested/empty"]
    );
}

#[test]
fn sg_fs_02_tree_rejects_file_count_individual_and_total_byte_limits_before_allocation() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("a"), b"aaaa").unwrap();
    fs::write(fixture.0.join("b"), b"bbbb").unwrap();
    for bound in [
        TreeLimits {
            files: 1,
            ..limits()
        },
        TreeLimits {
            file_bytes: 3,
            ..limits()
        },
        TreeLimits {
            total_bytes: 7,
            ..limits()
        },
        TreeLimits {
            entries: 1,
            ..limits()
        },
        TreeLimits {
            total_bytes: 64 * 1024 * 1024 + 1,
            ..limits()
        },
    ] {
        assert!(matches!(
            fixture.directory().capture(bound, |_| true),
            Err(FsError::Limit)
        ));
    }
    assert!(
        fixture
            .directory()
            .capture(
                TreeLimits {
                    files: 2,
                    file_bytes: 4,
                    total_bytes: 8,
                    entries: 2,
                    ..limits()
                },
                |_| true
            )
            .is_ok()
    );
}

#[test]
fn sg_fs_02_tree_counts_unselected_entries_toward_depth_and_entry_limits() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("a/b")).unwrap();
    fs::write(fixture.0.join("a/b/ignored.txt"), b"ignored").unwrap();
    let root = fixture.directory();
    for bound in [
        TreeLimits {
            depth: 1,
            ..limits()
        },
        TreeLimits {
            entries: 2,
            ..limits()
        },
    ] {
        assert!(matches!(
            root.capture(bound, |_| false),
            Err(FsError::Limit)
        ));
    }
    assert!(
        root.capture(
            TreeLimits {
                depth: 2,
                entries: 3,
                ..limits()
            },
            |_| false
        )
        .is_ok()
    );
    symlink("absent", fixture.0.join("unselected-link")).unwrap();
    assert!(matches!(
        root.capture(limits(), |_| false),
        Err(FsError::Path)
    ));
}

#[test]
fn fs02_tree_rejects_changes_after_reading_files_or_ancestor_entries() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("a"), b"original").unwrap();
    fs::write(fixture.0.join("b"), b"second").unwrap();
    let root = fixture.directory();
    let changed = root.capture_with_hook(
        limits(),
        |_| true,
        |path| {
            if path == "b" {
                fs::write(fixture.0.join("a"), b"changed!").unwrap();
            }
        },
    );
    assert!(matches!(changed, Err(FsError::Changed)));
    let added = root.capture_with_hook(
        limits(),
        |_| true,
        |path| {
            if path == "b" {
                fs::write(fixture.0.join("extra"), b"new").unwrap();
            }
        },
    );
    assert!(matches!(added, Err(FsError::Changed)));
}

#[test]
fn sg_fs_02_tree_detects_nested_directory_replacement_during_traversal() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("nested")).unwrap();
    fs::write(fixture.0.join("nested/a"), b"first").unwrap();
    let result = fixture.directory().capture_with_hook(
        limits(),
        |_| true,
        |_| {
            fs::rename(fixture.0.join("nested"), fixture.0.join("retained")).unwrap();
            fs::create_dir(fixture.0.join("nested")).unwrap();
        },
    );
    assert!(matches!(result, Err(FsError::Changed)));
    assert_eq!(fs::read(fixture.0.join("retained/a")).unwrap(), b"first");
}
