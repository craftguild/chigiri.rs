//! License metadata belongs to the generated package's caller, not its runtime.

use super::{MANIFEST_TEMPLATE, Manifest};

fn source(setting: &str) -> String {
    format!(
        "{}\n[runtime]\nsource=\"path\"\nroot=\".\"\n",
        MANIFEST_TEMPLATE.replace(
            "edition = \"2024\"",
            &format!("edition = \"2024\"\n{setting}")
        )
    )
}

#[test]
fn package_license_is_optional_and_preserves_explicit_caller_text() {
    assert_eq!(
        Manifest::parse("config", &source(""))
            .unwrap()
            .package()
            .license,
        None
    );
    for license in ["MIT", "MIT OR Apache-2.0", "LicenseRef-Application"] {
        let manifest = Manifest::parse("config", &source(&format!("license={license:?}"))).unwrap();
        assert_eq!(manifest.package().license.as_deref(), Some(license));
    }
}

#[test]
fn package_license_rejects_empty_control_wrong_type_and_unsupported_file_inputs() {
    for setting in [
        "license=\"\"",
        "license=\"   \"",
        "license=\"MIT\\nsecret\"",
        "license=true",
        "license=[\"MIT\"]",
        "license={name=\"MIT\"}",
        "license-file=\"LICENSE\"",
    ] {
        let error = Manifest::parse("config", &source(setting)).unwrap_err();
        assert_eq!(error.report().diagnostics()[0].code(), "SF4011");
        assert!(!error.to_string().contains("secret"));
    }
    let oversized = format!("license=\"{}\"", "x".repeat(4097));
    assert!(Manifest::parse("config", &source(&oversized)).is_err());
}
