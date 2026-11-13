//! Verify only the current format is created or read and legacy-format operations preserve every user file.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::test_support::Fixture;
use super::{generate_project, init_project, read_project_schema, update_project};

pub(super) const SDL: &str = "type A{code:String! peers:[B!]! extra:Int @virtual} type B{score:Int!} type Query{count:Int!} type Mutation{put:Int!}";

pub(super) fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn collect(root: &Path, directory: &Path, result: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_str().unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    collect(root, root, &mut result);
    result
}

#[test]
fn sd_formats_new_project_has_all_automatic_model_seeds_manual_modules_and_current_metadata() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let destination = fixture.base.join("new");
    init_project(&[fixture.base.join("schema.graphql")], &destination).unwrap();
    let schema = read_project_schema(&destination).unwrap();
    assert_eq!(schema.ir().models().len(), 2);
    let first = generate_project(schema.ir().clone()).unwrap();
    let second = generate_project(schema.ir().clone()).unwrap();
    assert_eq!(first.files, second.files);
    assert!(
        fs::read_to_string(destination.join("mock.luau"))
            .unwrap()
            .contains("chigiri-mock-project/v10")
    );
    for path in ["seeds/A.jsonl", "seeds/B.jsonl"] {
        assert_eq!(first.files[path], "");
    }
    for path in [
        "resolvers/query/count.luau",
        "resolvers/mutation/put.luau",
        "resolvers/objects/A/extra.luau",
    ] {
        assert!(first.files[path].contains("NOT_IMPLEMENTED"));
        assert!(!first.files[path].contains("models."));
    }
    for (path, expected) in [
        (
            "generated/update-state.json",
            "chigiri-mock-update-state/v11",
        ),
        (
            "generated/schema.snapshot.json",
            "chigiri-mock-schema-snapshot/v11",
        ),
    ] {
        let value: serde_json::Value = serde_json::from_str(&first.files[path]).unwrap();
        assert_eq!(value["format"], expected);
    }
    let state: serde_json::Value =
        serde_json::from_str(&first.files["generated/update-state.json"]).unwrap();
    assert_eq!(state["config"]["clock_mode"], "system");
    assert!(state["config"]["clock_now"].is_null());
    assert_eq!(
        state["config"]["capability_version"],
        "chigiri-mock-capabilities/v4"
    );
}

#[test]
fn sd_formats_old_project_and_metadata_are_rejected_without_replacing_user_or_recovery_files() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    fixture.write(
        "resolvers/query/count.luau",
        "U is not implemented by the user.",
    );
    fixture.write("resolvers/query/count.luau.dist", "Not adopted.");
    fixture.write("backup/original.luau", "original recovery evidence");
    fixture.write("proposal/candidate/t0000", "Unemployed candidate.");
    fixture.write("seeds/B.jsonl", "invalid seed data");
    let config = fixture.read("mock.luau");
    for version in [
        "pythia-mock-project/v7",
        "chigiri-mock-project/v1",
        "chigiri-mock-project/v2",
        "chigiri-mock-project/v3",
        "chigiri-mock-project/v4",
        "chigiri-mock-project/v5",
        "chigiri-mock-project/v6",
        "chigiri-mock-project/v7",
        "chigiri-mock-project/v8",
        "chigiri-mock-project/v9",
    ] {
        fixture.write(
            "mock.luau",
            &config.replace("chigiri-mock-project/v10", version),
        );
        let before = files(&fixture.project);
        assert!(read_project_schema(&fixture.project).is_err());
        assert!(update_project(&fixture.project).is_err());
        assert_eq!(files(&fixture.project), before);
    }
    fixture.write("mock.luau", &config);
    for (path, current, prefix) in [
        (
            "generated/update-state.json",
            "chigiri-mock-update-state/v11",
            "chigiri-mock-update-state",
        ),
        (
            "generated/schema.snapshot.json",
            "chigiri-mock-schema-snapshot/v11",
            "chigiri-mock-schema-snapshot",
        ),
    ] {
        let original = fixture.read(path);
        fixture.write(
            path,
            &original.replace(current, &current.replace("chigiri", "pythia")),
        );
        let before = files(&fixture.project);
        let error = update_project(&fixture.project).err().unwrap();
        assert_eq!(error.message, "unsupported generated metadata format");
        assert_eq!(files(&fixture.project), before);
        for version in 1..=10 {
            fixture.write(
                path,
                &original.replace(current, &format!("{prefix}/v{version}")),
            );
            let before = files(&fixture.project);
            let error = update_project(&fixture.project).err().unwrap();
            assert_eq!(error.message, "unsupported generated metadata format");
            assert_eq!(files(&fixture.project), before);
        }
        fixture.write(path, &original);
    }
    let before = files(&fixture.project);
    let destination = fixture.base.join("manual-migration");
    init_project(&[fixture.base.join("schema.graphql")], &destination).unwrap();
    assert_eq!(files(&fixture.project), before);
    assert!(destination.join("mock.luau").exists());
}
