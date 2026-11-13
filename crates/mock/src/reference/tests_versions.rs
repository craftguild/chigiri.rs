//! Verify version declarations against fixed oracles and reject legacy reference readers.

use serde_json::json;

use super::{Format, ReferenceDocument};

#[test]
fn sd_formats_reference_rejects_all_retired_versions_including_embedded_format_names() {
    let document = ReferenceDocument::build(None).unwrap();
    let original = document.render(Format::Json).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    assert_eq!(value["format"], "chigiri-mock-reference/v13");
    assert_eq!(value["api_version"], "chigiri-mock-api/v10");
    assert_eq!(
        value["supported_formats"],
        json!({"project":"chigiri-mock-project/v10","bundle":"chigiri-mock/v11"})
    );
    serde_json::from_slice::<ReferenceDocument>(&original).unwrap();
    for (field, prefix, last) in [
        ("format", "chigiri-mock-reference", 12),
        ("api_version", "chigiri-mock-api", 9),
    ] {
        for version in 1..=last {
            let mut changed = value.clone();
            changed[field] = json!(format!("{prefix}/v{version}"));
            assert!(serde_json::from_value::<ReferenceDocument>(changed).is_err());
        }
    }
    for (field, prefix, last) in [
        ("project", "chigiri-mock-project", 9),
        ("bundle", "chigiri-mock", 10),
    ] {
        for version in 1..=last {
            let name = format!("{prefix}/v{version}");
            let mut changed = value.clone();
            changed["supported_formats"][field] = json!(name);
            assert!(serde_json::from_value::<ReferenceDocument>(changed).is_err());
            let mut changed = value.clone();
            changed["mock_format"] = json!(name);
            assert!(serde_json::from_value::<ReferenceDocument>(changed).is_err());
        }
    }
}
