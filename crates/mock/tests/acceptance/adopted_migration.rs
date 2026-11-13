//! Exercise the format cutover through a fresh destination, relocation, and portable export.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::support::{Directory, Server};

const PUBLIC: &str = r#"
"Item model"
type Item {
  "Item identifier"
  id: ID!
  "Display title"
  title: String!
}

type Query {
  "List the items"
  items: [Item!]!
}
"#;
const MODULE: &str = "resolvers/query/items.luau";
const RESOLVER: &str = r#"--!strict
local resolver: ChigiriQueryItemsResolver = {
    resolve = function(ctx: ChigiriQueryItemsContext, _args: ChigiriQueryItemsArgs): ChigiriQueryItemsResult
        return ctx.models.Item:find({})
    end,
}
return resolver
"#;
const SEED: &str = "{\"id\":\"item-1\",\"title\":\"Migrated\",\"note\":\"private\"}\n";

fn init(directory: &Directory, name: &str) {
    let output = directory.command(&["init", "--schema", "schema.graphql", "--config-dir", name]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_rejected_without_snapshot_write(directory: &Directory, command: &str) {
    let snapshot = directory.0.join("legacy/generated/schema.snapshot.json");
    let before = fs::read(&snapshot).unwrap();
    let output = directory.command(&[command, "--config-dir", "legacy"]);
    assert_eq!(output.status.code(), Some(1), "{command}");
    assert!(output.stdout.is_empty(), "{command}");
    assert_eq!(fs::read(snapshot).unwrap(), before, "{command}");
}

fn json_output(directory: &Directory, arguments: &[&str]) -> Value {
    let output = directory.command(arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(root: &Path, current: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &entry.path(), files);
            } else {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }

    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    files
}

fn assert_legacy_command_rejected(directory: &Directory, arguments: &[&str]) {
    let project = directory.0.join("project");
    let before = tree(&project);
    let output = directory.command(arguments);
    assert_eq!(output.status.code(), Some(1), "{arguments:?}");
    assert!(output.stdout.is_empty(), "{arguments:?}");
    assert_eq!(tree(&project), before, "{arguments:?}");
}

#[test]
fn ev_12_product_update_rejects_the_old_protected_fixture_without_writes() {
    let _test_case = crate::support::test_case();
    let directory = Directory::new();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../mock/src/project/legacy_rejection_tests/fixtures/v8");
    copy_tree(&source, &directory.0);
    let before = tree(&directory.0);
    assert_eq!(before, tree(&source));

    for arguments in [
        vec!["update", "--config-dir", "project"],
        vec!["check", "--config-dir", "project"],
        vec!["codegen", "--config-dir", "project"],
        vec!["reference", "--config-dir", "project", "--format", "json"],
        vec!["export", "--config-dir", "project", "--output", "old.toml"],
        vec!["serve", "--config-dir", "project"],
    ] {
        assert_legacy_command_rejected(&directory, &arguments);
        // The command harness writes stdout/stderr beside the project. Its
        // project-tree check above includes every old owned/authored file;
        // acquisition input must remain unchanged as well.
        assert_eq!(
            fs::read(directory.0.join("schema.graphql")).unwrap(),
            before[Path::new("schema.graphql")]
        );
    }
    assert!(!directory.0.join("old.toml").exists());
}

#[test]
fn as_10_fresh_migration_relocates_without_public_input_and_matches_export() {
    let _test_case = crate::support::test_case();
    let source = Directory::new();
    fs::write(source.0.join("schema.graphql"), PUBLIC).unwrap();
    init(&source, "legacy");
    fs::write(source.0.join("legacy").join(MODULE), RESOLVER).unwrap();
    fs::write(source.0.join("legacy/seeds/Item.jsonl"), SEED).unwrap();

    let config_path = source.0.join("legacy/mock.luau");
    let current_config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        current_config.replace("chigiri-mock-project/v10", "chigiri-mock-project/v8"),
    )
    .unwrap();
    assert_rejected_without_snapshot_write(&source, "update");
    assert_rejected_without_snapshot_write(&source, "codegen");

    fs::write(&config_path, &current_config).unwrap();
    let state_path = source.0.join("legacy/generated/update-state.json");
    let state = fs::read_to_string(&state_path).unwrap();
    fs::write(
        &state_path,
        state.replace(
            "chigiri-mock-update-state/v11",
            "chigiri-mock-update-state/v8",
        ),
    )
    .unwrap();
    assert_rejected_without_snapshot_write(&source, "update");
    assert_rejected_without_snapshot_write(&source, "codegen");

    init(&source, "next");
    fs::write(
        source.0.join("next/storage.graphql"),
        "extend type Item { note: String }\n",
    )
    .unwrap();
    let output = source.command(&["codegen", "--config-dir", "next"]);
    assert!(output.status.success());
    fs::copy(
        source.0.join("legacy").join(MODULE),
        source.0.join("next").join(MODULE),
    )
    .unwrap();
    fs::copy(
        source.0.join("legacy/seeds/Item.jsonl"),
        source.0.join("next/seeds/Item.jsonl"),
    )
    .unwrap();
    let next_config = source.0.join("next/mock.luau");
    let config = fs::read_to_string(&next_config).unwrap();
    fs::write(next_config, config.replace("127.0.0.1:4000", "127.0.0.1:0")).unwrap();
    assert!(
        source
            .command(&["check", "--config-dir", "next"])
            .status
            .success()
    );

    let relocated = Directory::new();
    fs::rename(source.0.join("next"), relocated.0.join("mock")).unwrap();
    fs::remove_file(source.0.join("schema.graphql")).unwrap();
    assert!(!relocated.0.join("schema.graphql").exists());
    let project = json_output(
        &relocated,
        &["reference", "--config-dir", "mock", "--format", "json"],
    );
    assert!(
        relocated
            .command(&["check", "--config-dir", "mock"])
            .status
            .success()
    );
    assert!(
        relocated
            .command(&[
                "export",
                "--config-dir",
                "mock",
                "--output",
                "portable.toml",
            ])
            .status
            .success()
    );
    let bundle_reference = json_output(
        &relocated,
        &["reference", "--config", "portable.toml", "--format", "json"],
    );
    assert_eq!(project["schema"], bundle_reference["schema"]);
    assert_eq!(
        project["schema"]["models"][0]["description"]["text"],
        "Item model"
    );
    assert_eq!(
        project["schema"]["slots"][0]["description"]["text"],
        "Item identifier"
    );
    assert_eq!(
        project["schema"]["resolvers"][0]["description"]["text"],
        "List the items"
    );

    let bundle = fs::read_to_string(relocated.0.join("portable.toml")).unwrap();
    let query = "{items{id title}}";
    let introspection = "{__type(name:\"Item\"){description fields{name description}}}";
    let mut live = Server::project_with_graphiql(relocated);
    let expected = json!({"data":{"items":[{"id":"item-1","title":"Migrated"}]}});
    assert_eq!(live.graphql(query), expected);
    let live_schema = live.graphql(introspection);
    assert_eq!(live_schema["data"]["__type"]["description"], "Item model");
    assert!(live_schema.to_string().contains("Display title"));
    assert!(!live_schema.to_string().contains("note"));
    live.stop("-TERM");

    let mut portable = Server::with_graphiql(&bundle, true);
    assert_eq!(portable.graphql(query), expected);
    assert_eq!(portable.graphql(introspection), live_schema);
    portable.stop("-TERM");
}
