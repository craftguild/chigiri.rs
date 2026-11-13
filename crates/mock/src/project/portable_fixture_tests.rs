//! Keep the checked portable fixture owned by the current project producer.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use toml_edit::DocumentMut;

use super::test_support::Fixture;
use super::{Layout, export_project, read_project_schema, update_project};

const BLOG: &str = include_str!("../../fixtures/blog.graphql");
const BLUEPRINT: &str = include_str!("../../fixtures/mock-v9-ready.toml");
const CURRENT: &str = include_str!("../../fixtures/mock-v11-ready.toml");
const REGENERATE: &str = "CHIGIRI_REGENERATE_MOCK_PORTABLE_FIXTURE";

#[test]
fn canonical_portable_fixture_is_producer_owned() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    let blueprint = BLUEPRINT.parse::<DocumentMut>().unwrap();
    let schema = read_project_schema(&fixture.project).unwrap();
    let layout = Layout::new(schema.ir()).unwrap();
    drop(schema);
    install_config(&fixture);
    install_resolvers(&fixture, &layout, &blueprint);
    install_seeds(&fixture, &blueprint);
    update_project(&fixture.project).unwrap();
    let output = fixture.base.join("portable.toml");
    export_project(&fixture.project, &output).unwrap();
    let produced = fs::read_to_string(output).unwrap();
    if std::env::var_os(REGENERATE).is_some() {
        fs::write(destination(), &produced).unwrap();
        return;
    }
    assert_eq!(produced, CURRENT);
}

fn install_config(fixture: &Fixture) {
    let config = fixture
        .read("mock.luau")
        .replace("127.0.0.1:0", "127.0.0.1:4000")
        .replace(
            "allowed_origins = {}",
            "allowed_origins = {\"http://localhost:5173\"}",
        );
    fixture.write("mock.luau", &config);
}

fn install_resolvers(fixture: &Fixture, layout: &Layout, blueprint: &DocumentMut) {
    for (coordinate, entry) in blueprint["resolvers"].as_table().unwrap() {
        let path = layout
            .resolvers
            .iter()
            .find(|resolver| resolver.coordinate == coordinate)
            .unwrap()
            .path
            .as_str();
        fixture.write(path, entry["luau"].as_str().unwrap());
    }
}

fn install_seeds(fixture: &Fixture, blueprint: &DocumentMut) {
    let records: serde_json::Map<String, Value> =
        serde_json::from_str(blueprint["seed"]["records_json"].as_str().unwrap()).unwrap();
    for (model, records) in records {
        let records = records.as_array().unwrap();
        let mut jsonl = String::new();
        for record in records {
            jsonl.push_str(&serde_json::to_string(record).unwrap());
            jsonl.push('\n');
        }
        fixture.write(&format!("seeds/{model}.jsonl"), &jsonl);
    }
}

fn destination() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/mock-v11-ready.toml")
}
