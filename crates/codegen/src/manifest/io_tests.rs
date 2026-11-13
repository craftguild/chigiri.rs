//! Captures manifests and verifies that init changes no pre-existing entries.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{MANIFEST_TEMPLATE, Manifest, ResolvedManifest, init_manifest};
use crate::filesystem::Directory;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "chigiri-manifest-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }

    fn manifest(&self) -> PathBuf {
        self.0.join("chigiri.toml")
    }

    fn prepare(&self) -> String {
        let source =
            format!("{MANIFEST_TEMPLATE}\n[runtime]\nsource=\"path\"\nroot=\"../lab01\"\n");
        fs::create_dir(self.0.join("graphql")).unwrap();
        fs::write(self.manifest(), &source).unwrap();
        source
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sg_cli_02_init_creates_only_the_canonical_template_and_requires_runtime_selection() {
    let first = Fixture::new();
    let second = Fixture::new();
    for fixture in [&first, &second] {
        init_manifest(&fixture.manifest()).unwrap();
        let source = fs::read_to_string(fixture.manifest()).unwrap();
        assert_eq!(source, MANIFEST_TEMPLATE);
        assert!(Manifest::parse("chigiri.toml", &source).is_err());
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
        for runtime in [
            "[runtime]\nsource=\"path\"\nroot=\"../lab01\"",
            "[runtime]\nsource=\"git\"\nurl=\"ssh://git@example.org/chigiri.git\"\nrevision=\"bf3a360c185382e1030082ab1a9dbd656aaba9aa\"",
        ] {
            Manifest::parse("chigiri.toml", &format!("{source}\n{runtime}")).unwrap();
        }
    }
}

#[test]
fn sg_cli_02_init_preserves_existing_entries_and_rejects_parent_symlinks() {
    let fixture = Fixture::new();
    fs::write(fixture.manifest(), b"owner bytes").unwrap();
    assert!(init_manifest(&fixture.manifest()).is_err());
    assert_eq!(fs::read(fixture.manifest()).unwrap(), b"owner bytes");
    fs::create_dir(fixture.0.join("directory")).unwrap();
    assert!(init_manifest(&fixture.0.join("directory")).is_err());
    symlink("absent", fixture.0.join("link")).unwrap();
    assert!(init_manifest(&fixture.0.join("link")).is_err());
    assert_eq!(
        fs::read_link(fixture.0.join("link")).unwrap(),
        PathBuf::from("absent")
    );
    symlink("directory", fixture.0.join("parent-link")).unwrap();
    assert!(init_manifest(&fixture.0.join("parent-link/new.toml")).is_err());
    assert!(init_manifest(&fixture.0.join("absent/new.toml")).is_err());
    assert!(
        fs::read_dir(fixture.0.join("directory"))
            .unwrap()
            .next()
            .is_none()
    );
    assert!(!fixture.0.join("absent").exists());
}

#[test]
fn sg_fs_02_manifest_debug_output_excludes_captured_bytes_and_raw_sdl() {
    let fixture = Fixture::new();
    let source = fixture.prepare();
    let resolved = ResolvedManifest::load(&fixture.manifest()).unwrap();
    fs::write(fixture.manifest(), b"changed after capture").unwrap();
    assert_eq!(resolved.manifest_bytes(), source.as_bytes());
    assert_eq!(resolved.manifest_label(), "chigiri.toml");
    assert!(!format!("{resolved:?}").contains("../lab01"));
    assert!(resolved.verify_paths().is_err());
    fs::write(fixture.manifest(), &source).unwrap();
    resolved.verify_paths().unwrap();
    fs::rename(fixture.0.join("graphql"), fixture.0.join("retained")).unwrap();
    fs::create_dir(fixture.0.join("graphql")).unwrap();
    assert!(resolved.verify_paths().is_err());
    assert!(!fixture.0.join("generated").exists());
}

#[test]
fn sg_fs_02_rejects_manifest_and_sdl_symlinks_and_oversized_inputs() {
    let fixture = Fixture::new();
    fixture.prepare();
    symlink("chigiri.toml", fixture.0.join("linked.toml")).unwrap();
    assert!(ResolvedManifest::load(&fixture.0.join("linked.toml")).is_err());
    fs::rename(fixture.0.join("graphql"), fixture.0.join("real")).unwrap();
    symlink("real", fixture.0.join("graphql")).unwrap();
    assert!(ResolvedManifest::load(&fixture.manifest()).is_err());
    fs::write(fixture.manifest(), vec![b'#'; 65537]).unwrap();
    assert!(ResolvedManifest::load(&fixture.manifest()).is_err());
}

#[test]
fn sg_cli_05_resolves_the_consumer_from_the_manifest_root_and_rejects_output_overlap() {
    let fixture = Fixture::new();
    let source = fixture.prepare();
    fs::write(fixture.manifest(), format!(
        "{source}[consumer]\ncargo_manifest=\"app/Cargo.toml\"\npackage=\"app\"\ngenerated_dependency=\"contract\"\n"
    )).unwrap();
    fs::create_dir(fixture.0.join("app")).unwrap();
    let manifest = ResolvedManifest::load(&fixture.manifest()).unwrap();
    let (parent, name) = manifest.consumer_manifest().unwrap().unwrap();
    assert_eq!(name, "Cargo.toml");
    assert!(
        parent
            .same_location(&Directory::open(&fixture.0.join("app")).unwrap())
            .unwrap()
    );
    fs::write(
        fixture.manifest(),
        source.replace("generated/graphql-contract", "chigiri.toml"),
    )
    .unwrap();
    assert!(ResolvedManifest::load(&fixture.manifest()).is_err());
}

#[test]
fn sg_crate_06_preserves_the_original_schema_digest_during_handle_validation() {
    let fixture = Fixture::new();
    fixture.prepare();
    let schema = fixture.0.join("graphql");
    fs::create_dir(schema.join("nested")).unwrap();
    fs::write(schema.join("a.graphql"), "type Query { value: Int }\n").unwrap();
    fs::write(schema.join("nested/b.gql"), "scalar Date\n").unwrap();
    fs::write(schema.join("ignored.txt"), "ignored").unwrap();
    let manifest = ResolvedManifest::load(&fixture.manifest()).unwrap();
    assert_eq!(
        manifest.schema_digest().unwrap().to_hex(),
        "cccb9d5cb8a2598906514a939c9fe268a76502e3dab4e7bd35262c5bef9d121b"
    );
    fs::write(schema.join("a.graphql"), "invalid SDL").unwrap();
    assert!(manifest.schema_digest().is_err());
    fs::write(schema.join("a.graphql"), vec![b' '; 256 * 1024 + 1]).unwrap();
    assert!(manifest.schema_digest().is_err());
}
