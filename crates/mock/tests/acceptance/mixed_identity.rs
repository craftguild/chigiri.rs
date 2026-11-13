//! Reject exact bundle/capability generation mixes before reads, writes, or listening.

use std::fs;

use crate::support::{Directory, TEMPLATE};
use toml_edit::DocumentMut;

const LEGACY: &str = include_str!("../../fixtures/mock-v9-ready.toml");

fn mixed_generation_bundles() -> [(String, &'static str); 2] {
    let current_with_legacy_capability = TEMPLATE.replacen(
        "chigiri-mock-capabilities/v4",
        "chigiri-mock-capabilities/v2",
        1,
    );
    let legacy_with_current_capability = LEGACY.replacen(
        "chigiri-mock-capabilities/v2",
        "chigiri-mock-capabilities/v4",
        1,
    );
    assert_ne!(current_with_legacy_capability, TEMPLATE);
    assert_ne!(legacy_with_current_capability, LEGACY);
    let current = current_with_legacy_capability
        .parse::<DocumentMut>()
        .unwrap();
    let legacy = legacy_with_current_capability
        .parse::<DocumentMut>()
        .unwrap();
    assert_eq!(current["format"].as_str(), Some("chigiri-mock/v11"));
    assert_eq!(
        current["capabilities"].as_str(),
        Some("chigiri-mock-capabilities/v2")
    );
    assert_eq!(legacy["format"].as_str(), Some("chigiri-mock/v9"));
    assert_eq!(
        legacy["capabilities"].as_str(),
        Some("chigiri-mock-capabilities/v4")
    );
    [
        (current_with_legacy_capability, "v10-capability-v2.toml"),
        (legacy_with_current_capability, "v9-capability-v3.toml"),
    ]
}

#[test]
fn mixed_bundle_capability_generations_fail_every_product_reader() {
    let _test_case = crate::support::test_case();
    let current = Directory::new();
    fs::write(current.0.join("current.toml"), TEMPLATE).unwrap();
    for arguments in [
        vec!["check", "--config", "current.toml"],
        vec!["reference", "--config", "current.toml", "--format", "json"],
    ] {
        let output = current.command(&arguments);
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    for (source, name) in mixed_generation_bundles() {
        let directory = Directory::new();
        let path = directory.0.join(name);
        fs::write(&path, source.as_bytes()).unwrap();
        for arguments in [
            vec!["check", "--config", name],
            vec!["reference", "--config", name, "--format", "json"],
            vec!["serve", "--config", name],
        ] {
            let output = directory.command(&arguments);
            assert_eq!(output.status.code(), Some(1), "{arguments:?}");
            assert!(output.stdout.is_empty(), "{arguments:?}");
            assert_eq!(fs::read(&path).unwrap(), source.as_bytes(), "{arguments:?}");
        }
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 7);
    }
}
