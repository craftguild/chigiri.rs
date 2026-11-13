//! Verify exact Git package pins in published and private candidate manifests.

use std::fs;

use crate::contract::check::fixture::Fixture;
use crate::contract::render_recorded;
use crate::{MANIFEST_TEMPLATE, ResolvedManifest, check_contract, generator_info};

const URL: &str = "ssh://git@example.org/chigiri.git";
const REVISION: &str = "bf3a360c185382e1030082ab1a9dbd656aaba9aa";

fn configure(fixture: &mut Fixture, version: Option<&str>) {
    let version = version.map_or_else(String::new, |version| format!("version={version:?}"));
    fs::write(
        fixture.root.join("chigiri.toml"),
        format!(
            "{MANIFEST_TEMPLATE}\n[runtime]\nsource=\"git\"\nurl={URL:?}\nrevision={REVISION:?}\n{version}"
        ),
    )
    .unwrap();
    fixture.manifest = ResolvedManifest::load(&fixture.root.join("chigiri.toml")).unwrap();
}

#[test]
fn runtime_version_pins_git_without_changing_omitted_or_path_sources() {
    let mut fixture = Fixture::new();
    let path = super::render(&fixture.manifest).unwrap();
    let path = path.parse::<toml_edit::DocumentMut>().unwrap();
    assert!(path["dependencies"]["chigiri"].get("path").is_some());
    assert!(path["dependencies"]["chigiri"].get("version").is_none());

    for version in [None, Some("1.2.3"), Some("2.0.0-rc.1")] {
        configure(&mut fixture, version);
        let cargo = super::render(&fixture.manifest).unwrap();
        let private = super::render_at(&fixture.manifest, &fixture.manifest.root).unwrap();
        assert_eq!(cargo, private);
        let cargo = cargo.parse::<toml_edit::DocumentMut>().unwrap();
        let runtime = cargo["dependencies"]["chigiri"].as_inline_table().unwrap();
        assert_eq!(runtime["git"].as_str(), Some(URL));
        assert_eq!(runtime["rev"].as_str(), Some(REVISION));
        assert_eq!(runtime["package"].as_str(), Some("chigiri"));
        let expected = version.map(|value| format!("={value}"));
        assert_eq!(
            runtime.get("version").and_then(toml_edit::Value::as_str),
            expected.as_deref()
        );
    }
}

#[test]
fn runtime_version_round_trips_ownership_and_detects_drift_independently_of_generator() {
    let mut fixture = Fixture::new();
    configure(&mut fixture, Some("1.2.3"));
    let mut recorded = generator_info().clone();
    recorded.version = "9.8.7".into();
    let before = render_recorded(&fixture.manifest, &recorded).unwrap();
    fixture.write(&before);
    check_contract(&fixture.manifest).unwrap();

    configure(&mut fixture, Some("1.2.4"));
    let error = check_contract(&fixture.manifest).unwrap_err();
    assert_eq!(error.report().diagnostics()[0].code(), "SF4009");
    for (path, bytes) in before.files() {
        assert_eq!(fs::read(fixture.output().join(path)).unwrap(), bytes);
    }
    fixture.write(&render_recorded(&fixture.manifest, &recorded).unwrap());
    check_contract(&fixture.manifest).unwrap();
}
