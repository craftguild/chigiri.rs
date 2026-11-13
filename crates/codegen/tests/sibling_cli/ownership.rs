//! Verify CLI cleanup of semantic ownership units and rejection of forged marker ownership.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use super::fixture::Fixture;

#[test]
fn sg_fs_03_schema_shrink_removes_unused_ownership_shards_and_restore_artifacts() {
    let fixture = Fixture::new();
    let schema = fixture.root.join("graphql/schema.graphql");
    fs::write(&schema, "type Query { user: User } type User { id: ID! }").unwrap();
    fixture.success("codegen");
    let before = fixture.snapshot();
    let ownership = [
        "src/__chigiri/resolvers/objects/user.rs",
        "src/__chigiri/internal/bridge/objects/user.rs",
        "src/__chigiri/internal/coercion/user.rs",
        "src/__chigiri/internal/descriptor/types/user.rs",
        "src/__chigiri/internal/dispatch/user.rs",
    ];
    for path in ownership {
        assert!(
            before.contains_key(Path::new(path)),
            "missing initial {path}"
        );
    }
    fs::write(&schema, "type Query { value: String }").unwrap();
    fixture.success("update");
    fixture.success("check");
    let after = fixture.snapshot();
    for path in ownership {
        assert!(
            !after.contains_key(Path::new(path)),
            "retained obsolete {path}"
        );
    }
    let entries = fs::read_dir(fixture.output().parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(entries, ["contract"]);
}

#[test]
fn sg_fs_02_marker_symlinks_are_neither_tracked_nor_accepted_by_check_or_update() {
    let fixture = Fixture::new();
    fixture.success("codegen");
    let marker = fixture
        .output()
        .join(chigiri_codegen::GENERATED_MARKER_FILE);
    let outside = fixture.root.join("saved-marker.json");
    let saved = fs::read(&marker).unwrap();
    fs::rename(&marker, &outside).unwrap();
    symlink(&outside, &marker).unwrap();
    let before = fixture.snapshot();
    for command in ["check", "update"] {
        assert_eq!(fixture.run(command).status.code(), Some(1));
        assert!(marker.is_symlink());
        assert_eq!(fs::read(&outside).unwrap(), saved);
        assert_eq!(fixture.snapshot(), before);
    }
}
