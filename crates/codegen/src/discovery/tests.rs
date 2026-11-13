//! Verifies bounded SDL discovery through retained directory handles without
//! reopening paths by name.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use chigiri_core::ExecutionLimits;

use super::discover_captured;
use crate::filesystem::{Directory, FsError, TreeLimits};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "chigiri-discovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }

    fn capture(&self) -> Directory {
        Directory::open(&self.0).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn discover(root: &Directory) -> Result<Vec<super::DiscoveredSource>, crate::CodegenError> {
    discover_captured(root, &ExecutionLimits::default())
}

#[test]
fn discovers_only_schema_extensions_in_relative_path_order() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("nested")).unwrap();
    fs::write(fixture.0.join("z.graphql"), "type Z").unwrap();
    fs::write(fixture.0.join("nested/a.gql"), "type A").unwrap();
    fs::write(fixture.0.join("ignored.txt"), "type Ignored").unwrap();
    fs::write(fixture.0.join("upper.GRAPHQL"), "type Upper").unwrap();
    let sources = discover(&fixture.capture()).unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].relative_path, "nested/a.gql");
    assert_eq!(sources[0].contents, "type A");
    assert_eq!(sources[1].relative_path, "z.graphql");
    assert_eq!(sources[1].contents, "type Z");
}

#[test]
fn rejects_empty_source_sets_and_invalid_utf8_with_sf4002() {
    let fixture = Fixture::new();
    let error = discover(&fixture.capture()).unwrap_err();
    assert_eq!(error.report().diagnostics()[0].code(), "SF4002");
    assert!(
        error
            .to_string()
            .contains("contains no .graphql or .gql files")
    );
    fs::write(fixture.0.join("invalid.graphql"), [0xff]).unwrap();
    let error = discover(&fixture.capture()).unwrap_err();
    assert_eq!(error.report().diagnostics()[0].code(), "SF4002");
    assert!(error.to_string().contains("UTF-8"));
}

#[test]
fn reports_a_missing_workspace_root() {
    let fixture = Fixture::new();
    assert_eq!(
        Directory::open(&fixture.0.join("missing")).unwrap_err(),
        FsError::Missing
    );
}

#[test]
fn directory_listing_and_source_read_failures_preserve_the_filesystem_boundary() {
    let fixture = Fixture::new();
    let file = fixture.0.join("root.graphql");
    fs::write(&file, "type Query").unwrap();
    assert_eq!(Directory::open(&file).unwrap_err(), FsError::Path);
    assert_eq!(
        fixture.capture().read("missing.graphql", 1024).unwrap_err(),
        FsError::Missing
    );
}

#[test]
fn symlink_validation_rejects_multiple_paths_to_the_same_source() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("schema.graphql"), "type Query").unwrap();
    symlink("schema.graphql", fixture.0.join("alias.graphql")).unwrap();
    let error = discover(&fixture.capture()).unwrap_err();
    assert_eq!(error.report().diagnostics()[0].code(), "SF4002");
    assert!(error.to_string().contains("symlink"));
}

#[test]
fn rejects_source_byte_limits_before_allocating_the_full_input() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join("large.graphql"),
        "type Query { value: String }",
    )
    .unwrap();
    let limits = ExecutionLimits {
        max_document_bytes: 8,
        ..ExecutionLimits::default()
    };
    let error = discover_captured(&fixture.capture(), &limits).unwrap_err();
    assert_eq!(error.report().diagnostics()[0].code(), "SF4002");
    assert!(error.to_string().contains("filesystem resource limit"));
}

#[test]
fn enforces_directory_entry_limits_before_allocation() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("a.txt"), "selected entry count").unwrap();
    fs::write(fixture.0.join("schema.graphql"), "type Query").unwrap();
    let root = fixture.capture();
    let limits = TreeLimits {
        depth: 64,
        entries: 1,
        files: 1024,
        file_bytes: 256 * 1024,
        total_bytes: 16 * 1024 * 1024,
    };
    assert_eq!(
        root.capture(limits, |path| path.ends_with(".graphql"))
            .unwrap_err(),
        FsError::Limit
    );
    let captured = root
        .capture(
            TreeLimits {
                entries: 2,
                ..limits
            },
            |path| path.ends_with(".graphql"),
        )
        .unwrap();
    assert_eq!(captured.files.len(), 1);
}

#[test]
fn directory_depth_limit_returns_without_recursing_further() {
    let fixture = Fixture::new();
    let mut nested = fixture.0.clone();
    for index in 0..=super::MAX_DIRECTORY_DEPTH {
        nested.push(format!("level{index}"));
        fs::create_dir(&nested).unwrap();
    }
    let error = discover(&fixture.capture()).unwrap_err();
    assert_eq!(error.report().diagnostics()[0].code(), "SF4002");
    assert!(error.to_string().contains("filesystem resource limit"));
}

#[test]
fn ignores_untracked_file_and_directory_shapes() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    fs::write(fixture.0.join("real.graphql"), "type Real").unwrap();
    fs::write(outside.0.join("outside.graphql"), "type Outside").unwrap();
    for target in [outside.0.join("outside.graphql"), outside.0.clone()] {
        let link = fixture.0.join("linked");
        symlink(target, &link).unwrap();
        let error = discover(&fixture.capture()).unwrap_err();
        assert_eq!(error.report().diagnostics()[0].code(), "SF4002");
        assert!(error.to_string().contains("symlink"));
        fs::remove_file(link).unwrap();
    }
    assert_eq!(
        discover(&fixture.capture()).unwrap()[0].relative_path,
        "real.graphql"
    );
}

#[test]
fn rejects_symlink_roots() {
    let fixture = Fixture::new();
    let target = Fixture::new();
    symlink(&target.0, fixture.0.join("linked-root")).unwrap();
    assert_eq!(
        Directory::open(&fixture.0.join("linked-root")).unwrap_err(),
        FsError::Path
    );
    assert_eq!(
        fs::read_link(fixture.0.join("linked-root")).unwrap(),
        target.0
    );
}
