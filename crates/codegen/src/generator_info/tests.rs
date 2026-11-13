use super::{read_generator_info, write_generator_info};

const UNKNOWN: &str = r#"{
    "schema_version": "chigiri/generator-info-v1",
    "version": "0.1.0",
    "revision": "unknown",
    "git_commit": null,
    "dirty": null,
    "build": {
        "rustc": "unknown",
        "host": "aarch64-apple-darwin",
        "target": "aarch64-apple-darwin",
        "profile": "debug"
    }
}"#;

#[test]
fn sg_info_01_round_trips_every_field_between_json_and_the_rust_model() {
    let info = read_generator_info(UNKNOWN.as_bytes()).unwrap();
    let encoded = write_generator_info(&info).unwrap();
    assert_eq!(read_generator_info(encoded.as_bytes()).unwrap(), info);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
        serde_json::from_str::<serde_json::Value>(UNKNOWN).unwrap()
    );
}

#[test]
fn sg_info_05_nullable_including_all_keys_missing() {
    let original: serde_json::Value = serde_json::from_str(UNKNOWN).unwrap();
    for key in [
        "schema_version",
        "version",
        "revision",
        "git_commit",
        "dirty",
        "build",
    ] {
        let mut value = original.clone();
        value.as_object_mut().unwrap().remove(key);
        assert!(
            read_generator_info(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{key}"
        );
    }
    for key in ["rustc", "host", "target", "profile"] {
        let mut value = original.clone();
        value["build"].as_object_mut().unwrap().remove(key);
        assert!(
            read_generator_info(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{key}"
        );
    }
}

#[test]
fn sg_info_05_unknown_duplicate_schema_disagreement_with_secret_denial() {
    for document in [
        UNKNOWN.replacen('{', "{\"unexpected\": true,", 1),
        UNKNOWN.replace("\"build\": {", "\"build\": {\"unexpected\": true,"),
        UNKNOWN.replace("\"dirty\": null", "\"dirty\": null, \"dirty\": null"),
        UNKNOWN.replace(
            "\"profile\": \"debug\"",
            "\"profile\": \"debug\", \"profile\": \"debug\"",
        ),
        UNKNOWN.replace("generator-info-v1", "generator-info-v0"),
        UNKNOWN.replace("\"dirty\": null", "\"dirty\": false"),
        UNKNOWN.replace("\"debug\"", "\"https://token:secret@host/private\""),
        "x".repeat(4097),
    ] {
        let error = read_generator_info(document.as_bytes()).unwrap_err();
        assert!(!error.to_string().contains("secret"));
        assert_eq!(error.report().diagnostics()[0].code(), "SF4021");
    }
}
