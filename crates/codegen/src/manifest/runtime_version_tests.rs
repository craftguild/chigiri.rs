//! Keep optional runtime package pins distinct from source selection and package versions.

use super::{MANIFEST_TEMPLATE, Manifest, RuntimeSource};

fn source(version: &str) -> String {
    format!(
        "{MANIFEST_TEMPLATE}\n[runtime]\nsource=\"git\"\nurl=\"https://example.org/chigiri.git\"\nrevision=\"bf3a360c185382e1030082ab1a9dbd656aaba9aa\"\n{version}"
    )
}

#[test]
fn runtime_version_is_optional_and_independent_of_the_generated_package() {
    for version in [None, Some("1.2.3"), Some("2.0.0-rc.1")] {
        let setting = version.map_or_else(String::new, |value| format!("version={value:?}"));
        let manifest = Manifest::parse("chigiri.toml", &source(&setting)).unwrap();
        assert_eq!(manifest.package().version, "0.1.0");
        let RuntimeSource::Git {
            version: actual, ..
        } = manifest.runtime()
        else {
            panic!("expected Git runtime");
        };
        assert_eq!(actual.as_deref(), version);
    }
}

#[test]
fn runtime_version_rejects_requirements_invalid_values_and_path_settings() {
    for value in [
        "\"=0.1.0\"",
        "\"^0.1.0\"",
        "\"*\"",
        "\"0.1\"",
        "\"v0.1.0\"",
        "\"\"",
        "true",
        "[\"0.1.0\"]",
    ] {
        let source = source(&format!("version={value}"));
        let error = Manifest::parse("chigiri.toml", &source).unwrap_err();
        let diagnostic = &error.report().diagnostics()[0];
        assert_eq!(diagnostic.code(), "SF4011");
        assert_eq!(diagnostic.primary().unwrap().line(), source.lines().count());
    }
    let oversized = format!("1.2.3+{}", "a".repeat(256));
    assert!(Manifest::parse("chigiri.toml", &source(&format!("version={oversized:?}"))).is_err());
    let path =
        format!("{MANIFEST_TEMPLATE}\n[runtime]\nsource=\"path\"\nroot=\".\"\nversion=\"0.1.0\"");
    assert!(Manifest::parse("chigiri.toml", &path).is_err());
}
