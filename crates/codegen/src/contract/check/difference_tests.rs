//! Verifies that checks report file names, sort differences, and preserve the
//! manifest that existed before repair.

use std::fs;

use super::fixture::Fixture;
use crate::{
    DiagnosticFormat, ResolvedManifest, check_contract, render_contract, render_diagnostics,
};

#[test]
fn sg_fs_06_reports_changed_missing_and_added_files_by_name() {
    let fixture = Fixture::new();
    let rendered = render_contract(&fixture.manifest).unwrap();
    fixture.write(&rendered);
    let missing = rendered
        .files
        .keys()
        .find(|name| name.as_str() == "src/__chigiri/assembly.rs")
        .unwrap();
    fs::remove_file(fixture.output().join(missing.as_str())).unwrap();
    let cargo = fixture.output().join("Cargo.toml");
    let mut contents = fs::read(&cargo).unwrap();
    contents.extend_from_slice("\n # Changed \n".as_bytes());
    fs::write(cargo, contents).unwrap();
    fs::write(fixture.output().join("src/extra.rs"), "Added content").unwrap();
    let before = fixture.snapshot();
    let error = check_contract(&fixture.manifest).unwrap_err();
    let diagnostic = &error.report().diagnostics()[0];
    assert_eq!(diagnostic.code(), "SF4020");
    assert!(
        diagnostic
            .message()
            .contains("missing=1, extra=1, changed=1")
    );
    for expected in [
        format!("missing: {missing}"),
        "extra: src/extra.rs".into(),
        "changed: Cargo.toml".into(),
    ] {
        assert!(diagnostic.notes().contains(&expected), "{error}");
    }
    assert!(diagnostic.help()[0].contains("different name"));
    assert!(diagnostic.help()[0].contains("chigiri rust codegen"));
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn sg_fs_06_preserves_custom_manifest_paths_in_update_and_generate_guidance() {
    let mut fixture = Fixture::new();
    let name = "custom config's.toml";
    let manifest_path = fixture.root.join(name);
    fs::rename(fixture.root.join("chigiri.toml"), &manifest_path).unwrap();
    fixture.manifest = ResolvedManifest::load(&manifest_path).unwrap();
    fixture.write(&render_contract(&fixture.manifest).unwrap());
    fs::write(
        fixture.root.join("graphql/schema.graphql"),
        "# Change the input \n type Query {value: String}",
    )
    .unwrap();
    let before = fixture.snapshot();
    let drift = check_contract(&fixture.manifest).unwrap_err();
    let diagnostic = &drift.report().diagnostics()[0];
    assert_eq!(diagnostic.code(), "SF4009");
    assert!(
        diagnostic.help()[0].contains("chigiri rust --manifest 'custom config'\\''s.toml' update")
    );
    assert!(
        diagnostic
            .notes()
            .iter()
            .any(|note| note.starts_with("changed: "))
    );
    assert_eq!(fixture.snapshot(), before);

    fs::write(
        fixture.output().join("src/lib.rs"),
        "Change of ownership tree",
    )
    .unwrap();
    let before = fixture.snapshot();
    let invalid = check_contract(&fixture.manifest).unwrap_err();
    assert!(
        invalid.report().diagnostics()[0].help()[0]
            .contains("chigiri rust --manifest 'custom config'\\''s.toml' codegen")
    );
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn large_diffs_report_totals_truncate_details_and_redact_secret_file_names() {
    let fixture = Fixture::new();
    fixture.write(&render_contract(&fixture.manifest).unwrap());
    fs::write(fixture.output().join("password=do-not-print.rs"), "").unwrap();
    for index in 0..40 {
        fs::write(fixture.output().join(format!("z{index:02}.rs")), "").unwrap();
    }
    let before = fixture.snapshot();
    let error = check_contract(&fixture.manifest).unwrap_err();
    let diagnostic = &error.report().diagnostics()[0];
    assert!(error.report().truncated());
    assert!(diagnostic.message().contains("extra=41"));
    assert_eq!(diagnostic.notes().len(), 32);
    assert_eq!(diagnostic.notes()[1], "extra: <redacted>");
    for format in [DiagnosticFormat::Human, DiagnosticFormat::Json] {
        let report = render_diagnostics(error.report(), format);
        assert!(report.contains("SF4007"));
        assert!(!report.contains("do-not-print"));
        assert!(report.len() <= crate::MAX_DIAGNOSTIC_REPORT_BYTES);
    }
    assert!(!format!("{error:?}").contains("do-not-print"));
    assert_eq!(fixture.snapshot(), before);
}
