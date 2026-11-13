//! Transaction-test fixtures for observations before and after placement, including file bytes and empty directories.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::ProjectError;
use super::filesystem::TRANSACTION;
use super::test_support::Fixture;
use super::update_test_support::{MODULE, schema};

pub(super) fn fixture() -> Fixture {
    let fixture = Fixture::new(&schema("first"));
    fixture.write(MODULE, &(fixture.read(MODULE) + "n-- user editing n"));
    fixture.write("seeds/Item.jsonl", "invalid old seed\n");
    let next = schema("limit").replace(
        "type Query",
        "type Note { id:ID! label:String @virtual } type Query",
    );
    fs::write(fixture.base.join("schema.graphql"), next).unwrap();
    fixture
}

pub(super) fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, path: &Path, result: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap().to_str().unwrap();
            if relative.starts_with(TRANSACTION) {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                result.insert(format!("{relative}/"), Vec::new());
                visit(root, &path, result);
            } else {
                result.insert(relative.into(), fs::read(path).unwrap());
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

pub(super) fn operations(fixture: &Fixture) -> Vec<String> {
    let manifest: serde_json::Value =
        serde_json::from_str(&fixture.read(&format!("{TRANSACTION}/manifest.json"))).unwrap();
    manifest["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["path"].as_str().unwrap().to_owned())
        .collect()
}

pub(super) fn injected() -> ProjectError {
    ProjectError::new("environment", "injected transaction IO failure")
}

pub(super) fn recovery(fixture: &Fixture) -> serde_json::Value {
    serde_json::from_str(&fixture.read(&format!("{TRANSACTION}/recovery.json"))).unwrap()
}
