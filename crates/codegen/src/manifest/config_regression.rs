//! Regression tests preserving configuration spans, redaction, and input boundaries.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest as _, Sha256};

use super::{MANIFEST_TEMPLATE, Manifest, ResolvedManifest};
use crate::config::ResolvedBindings;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "chigiri-config-regression-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }

    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn source() -> String {
    format!("{MANIFEST_TEMPLATE}\n[runtime]\nsource=\"path\"\nroot=\".\"\n")
}

fn parse_error(source: &str) -> crate::CodegenError {
    Manifest::parse("chigiri.toml", source).unwrap_err()
}

#[test]
fn normalizes_manifest_paths_and_records_basename_and_exact_sha256() {
    let fixture = Fixture::new();
    let source = source();
    fixture.write("config/chigiri.toml", &source);
    fixture.write("config/graphql/schema.graphql", "type Query { value: Int }");
    let manifest = ResolvedManifest::load(&fixture.0.join("./config/./chigiri.toml")).unwrap();
    assert!(
        manifest
            .root
            .same_location(&crate::filesystem::Directory::open(&fixture.0.join("config")).unwrap())
            .unwrap()
    );
    assert_eq!(manifest.manifest_label(), "chigiri.toml");
    let compiled = manifest.compile().unwrap();
    let bindings = ResolvedBindings::from_manifest(&manifest, &compiled.schema).unwrap();
    let provenance = bindings.provenance().unwrap();
    assert_eq!(provenance.label(), "chigiri.toml");
    assert_eq!(
        provenance.digest_hex(),
        format!("{:x}", Sha256::digest(source.as_bytes()))
    );
    assert_eq!(provenance.digest_hex().len(), 64);
    assert!(
        provenance
            .digest_hex()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );

    // The CLI manifest selection is not limited to cwd. The input/output boundary is owned by the choice root.
    let alternate =
        ResolvedManifest::load(&fixture.0.join("config/../config/chigiri.toml")).unwrap();
    assert_eq!(alternate.manifest_bytes(), manifest.manifest_bytes());
    assert!(alternate.root.same_location(&manifest.root).unwrap());
}

#[test]
fn rejects_closed_shapes_and_syntax_errors_with_sf4011_spans() {
    let source = source();
    let cases = [
        source.replace("schema_version = \"chigiri/config-v2\"", ""),
        source.replace("chigiri/config-v2", "chigiri/config-v3"),
        format!("unknown = true\n{source}"),
        source.replace("[bindings]\ndomain_enums", "bindings.domain_enums"),
        source.replace("[bindings]", "[[bindings]]"),
        source.replace("domain_enums = []", "domain_enums = [1]"),
        format!("schema_version = \"chigiri/config-v2\"\n{source}"),
        "schema_version =\n".to_owned(),
    ];
    for source in cases {
        let error = parse_error(&source);
        let diagnostic = &error.report().diagnostics()[0];
        assert_eq!(diagnostic.code(), "SF4011", "{error}");
        let label = diagnostic.primary().unwrap();
        assert_eq!(label.path(), "chigiri.toml");
        assert!(label.line() > 0 && label.column() > 0);
        assert!(error.to_string().contains("manifest entry"));
        assert!(
            !error.to_string().contains(" | "),
            "the diagnostic must not expose raw source"
        );
    }
}

#[test]
fn counts_crlf_as_one_diagnostic_line_break() {
    let error = parse_error("schema_version = \"chigiri/config-v2\"\r\nunknown = true\r\n");
    let label = error.report().diagnostics()[0].primary().unwrap();
    assert_eq!(label.line(), 2);
    assert!(error.to_string().contains("chigiri.toml:2:"));
    assert!(!error.to_string().contains("chigiri.toml:3:"));
}

#[test]
fn rejects_invalid_and_duplicate_names_with_sf4012_spans() {
    for names in ["\"not-name\"", "\"Role\", \"Role\""] {
        let source = source().replace("[bindings]\ndomain_enums = []", "");
        let header = "schema_version = \"chigiri/config-v2\"";
        let rest = source.replace(header, "");
        let error = parse_error(&format!(
            "{header}\n[bindings]\ndomain_enums=[{names}]\n{rest}"
        ));
        let diagnostic = &error.report().diagnostics()[0];
        assert_eq!(diagnostic.code(), "SF4012", "{error}");
        assert_eq!(diagnostic.primary().unwrap().line(), 3);
        assert!(error.to_string().contains("manifest entry"));
        assert!(!error.to_string().contains(" | "));
    }
}

#[test]
fn rejects_directories_non_utf8_content_and_oversized_manifests() {
    let fixture = Fixture::new();
    fixture.write("directory/file", b"owned bytes");
    fixture.write("large.toml", vec![b' '; 64 * 1024 + 1]);
    for name in ["directory", "large.toml"] {
        let error = ResolvedManifest::load(&fixture.0.join(name)).unwrap_err();
        assert_eq!(error.report().diagnostics()[0].code(), "SF4011");
        assert!(error.to_string().contains(name), "{error}");
    }
    fixture.write("invalid.toml", [0xff]);
    let error = ResolvedManifest::load(&fixture.0.join("invalid.toml")).unwrap_err();
    assert!(error.to_string().contains("UTF-8"));
    assert!(error.to_string().contains("invalid.toml"));
    assert_eq!(
        fs::read(fixture.0.join("directory/file")).unwrap(),
        b"owned bytes"
    );
}

#[test]
fn rejects_symlinks_for_manifest_paths_and_components() {
    let fixture = Fixture::new();
    fixture.write("real/chigiri.toml", source());
    symlink("real/chigiri.toml", fixture.0.join("config.toml")).unwrap();
    symlink("real", fixture.0.join("linked")).unwrap();
    for name in ["config.toml", "linked/chigiri.toml"] {
        let error = ResolvedManifest::load(&fixture.0.join(name)).unwrap_err();
        assert_eq!(error.report().diagnostics()[0].code(), "SF4011");
        assert!(error.to_string().contains("symlink"));
    }
}
