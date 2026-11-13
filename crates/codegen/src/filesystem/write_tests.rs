//! Test owned-tree deletion, replacement, limits, and unknown files.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use super::tests::Fixture;
use super::{CapturedTree, FsError, TreeLimits};

#[test]
fn sg_fs_04_stream_enforces_byte_limits_and_existing_entries_and_leaves_failed_new_file() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    let result = root.create_stream("partial", 7, |writer| {
        writer.write(b"1234")?;
        writer.write(b"5678")
    });
    assert!(matches!(result, Err(FsError::Limit)));
    assert_eq!(root.read("partial", 10).unwrap(), b"1234");
    assert!(matches!(
        root.create_stream("partial", 100, |_| Ok(())),
        Err(FsError::Exists)
    ));
    assert!(matches!(
        root.create_stream("absent", 68 * 1024 * 1024 + 1, |_| Ok(())),
        Err(FsError::Limit)
    ));
    root.absent("absent").unwrap();
}

#[test]
fn sg_fs_04_rejects_archive_content_and_path_swaps_before_unlinking() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    let archive = root
        .create_stream("archive", 100, |writer| writer.write(b"old bytes"))
        .unwrap();
    fs::write(fixture.0.join("archive"), b"new bytes").unwrap();
    assert_eq!(archive.remove(), Err(FsError::Changed));
    let archive = root
        .create_stream("second", 100, |writer| writer.write(b"original"))
        .unwrap();
    fs::rename(fixture.0.join("second"), fixture.0.join("retained")).unwrap();
    fs::write(fixture.0.join("second"), b"attacker").unwrap();
    assert_eq!(archive.remove(), Err(FsError::Changed));
    assert_eq!(root.read("retained", 100).unwrap(), b"original");
    assert_eq!(root.read("second", 100).unwrap(), b"attacker");
    let tampered = root.create_stream("during", 100, |writer| {
        writer.write(b"original")?;
        fs::write(fixture.0.join("during"), b"modified").unwrap();
        Ok(())
    });
    assert!(matches!(tampered, Err(FsError::Changed)));
}

#[test]
fn sg_fs_04_atomic_claim_preserves_a_replacement_installed_after_verification() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    let archive = root
        .create_stream("raced", 100, |writer| writer.write(b"original"))
        .unwrap();
    assert_eq!(
        archive.remove_with_hook(|| {
            fs::rename(fixture.0.join("raced"), fixture.0.join("retained")).unwrap();
            fs::write(fixture.0.join("raced"), b"replacement").unwrap();
        }),
        Err(FsError::Changed)
    );
    assert_eq!(root.read("retained", 100).unwrap(), b"original");
    assert_eq!(root.read("raced", 100).unwrap(), b"replacement");
    assert!(fs::read_dir(&fixture.0).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".chigiri-remove-")
    }));
}

#[test]
fn sg_fs_04_private_claim_deletes_only_the_claimed_identity() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    let archive = root
        .create_stream("claimed", 100, |writer| writer.write(b"original"))
        .unwrap();
    archive
        .remove_with_hooks(
            || {},
            || fs::write(fixture.0.join("claimed"), b"replacement").unwrap(),
        )
        .unwrap();
    assert_eq!(root.read("claimed", 100).unwrap(), b"replacement");
    assert!(fs::read_dir(&fixture.0).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".chigiri-remove-")
    }));
}

#[test]
fn sg_fs_04_tree_preserves_all_unknown_files_and_deletes_only_known_trees() {
    let fixture = Fixture::new();
    let root = fixture.directory().create_directory("owned").unwrap();
    let tree = CapturedTree::try_from_files(BTreeMap::from([
        ("a/one".to_owned(), b"first".to_vec()),
        ("a/b/two".to_owned(), b"second".to_vec()),
    ]))
    .unwrap();
    root.write_tree(&tree).unwrap();
    root.create("unknown", b"keep", 100).unwrap();
    assert_eq!(
        root.remove_tree(&tree, &mut || Ok(())),
        Err(FsError::Changed)
    );
    assert_eq!(root.read("a/one", 100).unwrap(), b"first");
    assert_eq!(root.read("unknown", 100).unwrap(), b"keep");
    fs::remove_file(fixture.0.join("owned/unknown")).unwrap();
    root.remove_tree(&tree, &mut || Ok(())).unwrap();
    fixture.directory().absent("owned").unwrap();
}

#[test]
fn sg_fs_02_tree_rejects_entry_collisions_and_path_limits_before_writing_parts() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    for files in [
        BTreeMap::from([("../outside".to_owned(), vec![])]),
        BTreeMap::from([("a".to_owned(), vec![]), ("a/b".to_owned(), vec![])]),
        BTreeMap::from([(vec!["a"; 65].join("/"), vec![])]),
    ] {
        assert!(CapturedTree::try_from_files(files).is_err());
        assert!(
            root.capture(TreeLimits::generated(), |_| true)
                .unwrap()
                .files
                .is_empty()
        );
        root.absent("a").unwrap();
    }
}

#[test]
fn sg_proc_03_cache_allows_missing_targets_but_rejects_parent_links_and_files() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    root.create_directory("real").unwrap();
    root.create("file", b"old", 100).unwrap();
    symlink("real", fixture.0.join("link")).unwrap();
    assert_eq!(
        root.resolve_destination(Path::new("real/absent/../target"))
            .unwrap(),
        root.resolve_destination(Path::new("real/target")).unwrap()
    );
    assert!(root.resolve_destination(Path::new("link/target")).is_err());
    assert!(root.resolve_destination(Path::new("file/target")).is_err());
    assert!(!fixture.0.join("real/target").exists());
}

#[test]
fn sg_proc_03_overlap_is_determined_by_both_the_existing_inode_and_the_absent_suffix() {
    let fixture = Fixture::new();
    let root = fixture.directory();
    root.create_directory("schema").unwrap();
    root.create_directory("cache").unwrap();
    for (left, right, expected) in [
        ("schema", "schema/target", true),
        ("schema/target", "schema", true),
        ("schema", "cache", false),
        ("absent/sdk", "absent/SDK/target", true),
        ("absent/sdk", "absent/cache", false),
        ("absent/é", "absent/e\u{301}/target", true),
    ] {
        let left = root.resolve_destination(Path::new(left)).unwrap();
        let right = root.resolve_destination(Path::new(right)).unwrap();
        assert_eq!(root.overlaps_destination(&left, &right).unwrap(), expected);
    }
    root.create_directory("é").unwrap();
    if root.relative("e\u{301}").is_ok() {
        let left = root.resolve_destination(Path::new("é")).unwrap();
        let right = root
            .resolve_destination(Path::new("e\u{301}/target"))
            .unwrap();
        assert!(root.overlaps_destination(&left, &right).unwrap());
    }
}
