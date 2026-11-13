//! Preserve installed/private rendering and protected ownership for caller licensing.

use std::fs;

use crate::contract::check::fixture::Fixture;
use crate::{
    DiagnosticPolicy, ResolvedManifest, check_contract, generate_contract, update_contract,
};

fn configure(fixture: &mut Fixture, license: &str) {
    let path = fixture.root.join("chigiri.toml");
    let source = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        source.replace(
            "edition = \"2024\"",
            &format!("edition = \"2024\"\nlicense={license:?}"),
        ),
    )
    .unwrap();
    fixture.manifest = ResolvedManifest::load(&path).unwrap();
}

#[test]
fn package_license_changes_only_the_explicit_cargo_field() {
    let mut fixture = Fixture::new();
    let omitted = super::render(&fixture.manifest).unwrap();
    let omitted_private = super::render_at(&fixture.manifest, &fixture.manifest.root).unwrap();
    assert!(
        omitted.parse::<toml_edit::DocumentMut>().unwrap()["package"]
            .get("license")
            .is_none()
    );
    configure(&mut fixture, "MIT OR Apache-2.0");
    let installed = super::render(&fixture.manifest).unwrap();
    let private = super::render_at(&fixture.manifest, &fixture.manifest.root).unwrap();
    let package = installed.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(
        package["package"]["license"].as_str(),
        Some("MIT OR Apache-2.0")
    );
    assert_eq!(
        private.parse::<toml_edit::DocumentMut>().unwrap()["package"]["license"].as_str(),
        Some("MIT OR Apache-2.0")
    );
    assert_eq!(
        installed.replace("license = \"MIT OR Apache-2.0\"\n", ""),
        omitted
    );
    assert_eq!(
        private.replace("license = \"MIT OR Apache-2.0\"\n", ""),
        omitted_private
    );
}

#[test]
fn package_license_adoption_uses_protected_update_and_detects_manual_drift() {
    let mut fixture = Fixture::new();
    generate_contract(&fixture.manifest, DiagnosticPolicy::default()).unwrap();
    let cargo_path = fixture.output().join("Cargo.toml");
    let before = fs::read(&cargo_path).unwrap();
    configure(&mut fixture, "MIT");
    assert!(check_contract(&fixture.manifest).is_err());
    assert_eq!(fs::read(&cargo_path).unwrap(), before);
    update_contract(&fixture.manifest, DiagnosticPolicy::default()).unwrap();
    check_contract(&fixture.manifest).unwrap();
    let generated = fs::read_to_string(&cargo_path).unwrap();
    assert_eq!(
        generated.parse::<toml_edit::DocumentMut>().unwrap()["package"]["license"].as_str(),
        Some("MIT")
    );
    let edited = generated.replace("license = \"MIT\"", "license = \"Apache-2.0\"");
    fs::write(&cargo_path, &edited).unwrap();
    assert!(check_contract(&fixture.manifest).is_err());
    assert!(update_contract(&fixture.manifest, DiagnosticPolicy::default()).is_err());
    assert_eq!(fs::read_to_string(&cargo_path).unwrap(), edited);
}
