//! Verify scalar representation through project initialization, generated management files, bundles, and HTTP input.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::{init, ready};
use crate::project_workflow::project;
use crate::support::Server;
use serde_json::{Value, json};

const SDL: &str = r#"
scalar Count @representation(type: INTEGER, min: 0)
scalar Stamp @representation(type: STRING, minBytes: 2)
type Item { id: ID! count: Count! stamp: Stamp! }
type Query { items: [Item!]! }
type Mutation { add(count: Count!, stamp: Stamp!): Item! bad: Count! }
"#;

fn configured() -> String {
    let (_, config) = init(SDL);
    ready(
        &config,
        &[
            ("Query.items", "return ctx.models.Item:find({})"),
            (
                "Mutation.add",
                "return ctx.models.Item:create({id='Item-' .. (ctx.models.Item:count({}) + 1),count=args.count,stamp=args.stamp})",
            ),
            ("Mutation.bad", "return -1"),
        ],
    )
}

fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, found);
            } else {
                found.insert(
                    path.strip_prefix(root).unwrap().to_str().unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut found = BTreeMap::new();
    visit(root, root, &mut found);
    found
}

#[test]
fn rep_06_07_old_management_with_new_sdl_rejects_without_writing_user_files() {
    let _case = crate::support::test_case();
    let directory = project(SDL, &configured());
    let root = directory.0.join("mock");
    fs::write(
        root.join("resolvers/mutation/bad.luau.dist"),
        "Dist of the user.",
    )
    .unwrap();
    fs::create_dir(root.join("backup")).unwrap();
    fs::write(
        root.join("backup/original.luau"),
        "original recovery evidence",
    )
    .unwrap();
    for (path, current, old) in [
        (
            "generated/update-state.json",
            "chigiri-mock-update-state/v11",
            "chigiri-mock-update-state/v4",
        ),
        (
            "generated/schema.snapshot.json",
            "chigiri-mock-schema-snapshot/v11",
            "chigiri-mock-schema-snapshot/v4",
        ),
    ] {
        let original = fs::read_to_string(root.join(path)).unwrap();
        fs::write(root.join(path), original.replace(current, old)).unwrap();
        let before = files(&root);
        let result = directory.command(&["update", "--config-dir", "mock"]);
        assert_eq!(result.status.code(), Some(1));
        assert_eq!(files(&root), before);
        fs::write(root.join(path), original).unwrap();
    }
}

#[test]
fn rep_08_09_cli_project_export_and_http_scalar_failure_roll_back_all_roots() {
    let _case = crate::support::test_case();
    let directory = project(SDL, &configured());
    let root = directory.0.join("mock");
    for args in [
        &["check", "--config-dir", "mock"][..],
        &["update", "--config-dir", "mock"],
        &["check", "--config-dir", "mock"],
        &["export", "--config-dir", "mock", "--output", "bundle.toml"],
    ] {
        let output = directory.command(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let bundle = fs::read_to_string(directory.0.join("bundle.toml")).unwrap();
    assert!(bundle.contains("chigiri-mock-directives/v3"));
    assert!(bundle.contains("@representation("));
    assert!(
        fs::read_to_string(root.join("generated/types.d.luau"))
            .unwrap()
            .contains("ChigiriMutationAddArgs")
    );
    let mut server = Server::project(directory);
    let invalid_input = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":r#"mutation { add(count: -1, stamp: "x") { id } }"#}).to_string(),
    );
    assert!(matches!(invalid_input.status, 422 | 500));
    assert_eq!(server.graphql("{items{id}} ")["data"], json!({"items":[]}));
    assert_eq!(
        server.graphql(r#"mutation { add(count: 2, stamp: "ok") { id count stamp } }"#)["data"],
        json!({"add":{"id":"Item-1","count":2,"stamp":"ok"}})
    );
    let failed = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":r#"mutation { add(count: 3, stamp: "ok") { id } bad }"#}).to_string(),
    );
    assert_eq!(failed.status, 500);
    let body: Value = serde_json::from_str(&failed.body).unwrap();
    assert_eq!(body["data"], Value::Null);
    assert_eq!(
        body["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    assert_eq!(
        server.graphql("{items{id count}} ")["data"],
        json!({"items":[{"id":"Item-1","count":2}]})
    );
    assert_eq!(
        server.graphql(r#"mutation { add(count: 4, stamp: "go") { id } }"#)["data"],
        json!({"add":{"id":"Item-2"}})
    );
    server.stop("-TERM");
    fs::rename(
        server.directory.0.join("mock"),
        server.directory.0.join("old-project"),
    )
    .unwrap();
    fs::remove_file(server.directory.0.join("schema.graphql")).unwrap();
    let mut offline = Server::offline(&bundle);
    assert_eq!(offline.graphql("{items{id}} ")["data"], json!({"items":[]}));
    offline.stop("-TERM");
}
