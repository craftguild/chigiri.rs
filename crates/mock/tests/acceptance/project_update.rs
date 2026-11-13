//! Verifies real CLI layout and validation behavior, B/U/N migration proposals,
//! preservation of manual edits, and incomplete transaction markers.

use serde_json::Value;
use std::fs;

use crate::support::{Directory, Server};

const MODULE: &str = "mock/resolvers/query/items.luau";
const STATE: &str = "mock/generated/update-state.json";

fn schema(argument: &str) -> String {
    format!(
        "type Item {{id:ID! title:String!}} type Query {{items({argument}:Int! = 20):[Item!]!}}"
    )
}

fn update(directory: &Directory) -> String {
    let output = directory.command(&["update", "--config-dir", "mock"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("placement complete"));
    assert!(text.contains("validation_checked = false"));
    text
}

fn base(directory: &Directory) -> Value {
    let state: Value = serde_json::from_slice(&fs::read(directory.0.join(STATE)).unwrap()).unwrap();
    state["resolvers"][0]["base"].clone()
}

fn project() -> Directory {
    let directory = Directory::new();
    fs::write(directory.0.join("schema.graphql"), schema("first")).unwrap();
    let result = directory.command(&["init", "--schema", "schema.graphql", "--config-dir", "mock"]);
    assert!(result.status.success());
    let path = directory.0.join("mock/mock.luau");
    let config = fs::read_to_string(&path).unwrap();
    fs::write(path, config.replace("127.0.0.1:4000", "127.0.0.1:0")).unwrap();
    directory
}

#[test]
fn as_03_04_cli_codegen_uses_adopted_public_sdl_when_locator_is_missing() {
    let _test_case = crate::support::test_case();
    let directory = project();
    fs::remove_file(directory.0.join("schema.graphql")).unwrap();

    let codegen = directory.command(&["codegen", "--config-dir", "mock"]);
    assert!(
        codegen.status.success(),
        "{}",
        String::from_utf8_lossy(&codegen.stderr)
    );
    let text = String::from_utf8(codegen.stdout).unwrap();
    assert!(text.contains("chigiri mock codegen: placement complete"));
    assert!(
        directory
            .command(&["reference", "--config-dir", "mock"])
            .status
            .success()
    );
    assert!(
        directory
            .command(&["check", "--config-dir", "mock"])
            .status
            .success()
    );
}

#[test]
fn mp_09_10_11_12_16_update_proposal_base_manual_migration_and_http_readiness() {
    let _test_case = crate::support::test_case();
    let directory = project();
    let original_base = base(&directory);
    fs::write(directory.0.join("schema.graphql"), schema("limit")).unwrap();
    assert!(update(&directory).contains("replaced: resolvers/query/items.luau"));
    assert_ne!(base(&directory), original_base);
    let placed_base = base(&directory);
    fs::write(directory.0.join(MODULE), "invalid legacy resolver").unwrap();
    fs::write(
        directory.0.join("mock/seeds/Item.jsonl"),
        "invalid legacy seed",
    )
    .unwrap();
    fs::write(directory.0.join("schema.graphql"), schema("size")).unwrap();
    assert!(update(&directory).contains("proposed: resolvers/query/items.luau"));
    assert_eq!(base(&directory), placed_base);
    assert_eq!(
        fs::read_to_string(directory.0.join(MODULE)).unwrap(),
        "invalid legacy resolver"
    );
    let state = fs::read(directory.0.join(STATE)).unwrap();
    update(&directory);
    assert_eq!(fs::read(directory.0.join(STATE)).unwrap(), state);
    for arguments in [
        vec!["check", "--config-dir", "mock"],
        vec!["serve", "--config-dir", "mock"],
        vec!["export", "--config-dir", "mock", "--output", "bad.toml"],
    ] {
        let result = directory.command(&arguments);
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
    }
    assert!(!directory.0.join("bad.toml").exists());
    assert!(
        directory
            .command(&["reference", "--config-dir", "mock", "--format", "json"])
            .status
            .success()
    );
    fs::write(
        directory.0.join(MODULE),
        "--!strict\nlocal resolver: ChigiriQueryItemsResolver = { resolve=function(ctx: ChigiriQueryItemsContext, args: ChigiriQueryItemsArgs): ChigiriQueryItemsResult return {} end }\nreturn resolver",
    )
    .unwrap();
    fs::write(directory.0.join("mock/seeds/Item.jsonl"), "").unwrap();
    assert!(
        directory
            .command(&["check", "--config-dir", "mock"])
            .status
            .success()
    );
    update(&directory);
    assert_eq!(base(&directory), placed_base);
    assert!(
        directory
            .command(&[
                "export",
                "--config-dir",
                "mock",
                "--output",
                "portable.toml"
            ])
            .status
            .success()
    );
    let bundle = fs::read_to_string(directory.0.join("portable.toml")).unwrap();
    assert!(!bundle.contains(".dist"));
    let mut server = Server::project(directory);
    assert_eq!(
        server.graphql("{items{id}}")["data"]["items"],
        serde_json::json!([])
    );
    server.stop("-TERM");
}

#[test]
fn cv_08_new_operation_stub_preserves_edits_and_serves_after_bundle_restart() {
    let _test_case = crate::support::test_case();
    let directory = project();
    let resolver = "--!strict\nlocal resolver: ChigiriQueryItemsResolver = { resolve=function(ctx: ChigiriQueryItemsContext, args: ChigiriQueryItemsArgs): ChigiriQueryItemsResult return ctx.models.Item:find({}) end }\nreturn resolver";
    let seed = "{\"id\":\"item-1\",\"title\":\"Preserved\"}\n";
    fs::write(directory.0.join(MODULE), resolver).unwrap();
    fs::write(directory.0.join("mock/seeds/Item.jsonl"), seed).unwrap();
    fs::write(
        directory.0.join("schema.graphql"),
        format!(
            "{} extend type Query {{ pending: Boolean! }}",
            schema("first")
        ),
    )
    .unwrap();

    let report = update(&directory);
    assert!(report.contains("created: resolvers/query/pending.luau"));
    assert_eq!(
        fs::read_to_string(directory.0.join(MODULE)).unwrap(),
        resolver
    );
    assert_eq!(
        fs::read_to_string(directory.0.join("mock/seeds/Item.jsonl")).unwrap(),
        seed
    );
    assert!(!directory.0.join(format!("{MODULE}.dist")).exists());
    let pending =
        fs::read_to_string(directory.0.join("mock/resolvers/query/pending.luau")).unwrap();
    assert!(pending.contains("code = \"NOT_IMPLEMENTED\""));
    assert!(pending.contains("message = \"This operation is not implemented.\""));
    assert!(!pending.contains("state ="));
    assert!(
        directory
            .command(&["check", "--config-dir", "mock"])
            .status
            .success()
    );
    assert!(
        directory
            .command(&["export", "--config-dir", "mock", "--output", "updated.toml",])
            .status
            .success()
    );
    let bundle = fs::read_to_string(directory.0.join("updated.toml")).unwrap();

    for _ in 0..2 {
        let mut server = Server::start(&bundle);
        assert_eq!(
            server.graphql("{items{id title}}")["data"]["items"],
            serde_json::json!([{"id":"item-1","title":"Preserved"}])
        );
        let failed = server.graphql("{pending}");
        assert!(failed["data"].is_null());
        assert_eq!(failed["errors"][0]["path"], serde_json::json!(["pending"]));
        assert_eq!(failed["errors"][0]["extensions"]["code"], "NOT_IMPLEMENTED");
        assert_eq!(
            failed["errors"][0]["message"],
            "This operation is not implemented."
        );
        server.stop("-TERM");
    }
}

#[test]
fn mp_11_15_cli_adoption_stale_dist_and_transaction_marker_do_not_claim_readiness() {
    let _test_case = crate::support::test_case();
    let directory = project();
    for arguments in [
        vec![
            "export",
            "--config-dir",
            "mock",
            "--output",
            "missing/bundle.toml",
        ],
        vec![
            "init",
            "--schema",
            "missing.graphql",
            "--config-dir",
            "another",
        ],
    ] {
        let output = directory.command(&arguments);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    let old = fs::read_to_string(directory.0.join(MODULE)).unwrap();
    fs::write(directory.0.join(MODULE), format!("-- User edit\n{old}")).unwrap();
    fs::write(directory.0.join("schema.graphql"), schema("limit")).unwrap();
    assert!(update(&directory).contains("proposed: resolvers/query/items.luau"));
    assert!(
        fs::read_to_string(directory.0.join(MODULE))
            .unwrap()
            .starts_with("-- User edit")
    );
    fs::write(directory.0.join("schema.graphql"), schema("first")).unwrap();
    assert!(update(&directory).contains("kept: resolvers/query/items.luau"));
    fs::write(
        directory.0.join(MODULE),
        "--!strict\nlocal resolver: ChigiriQueryItemsResolver = { resolve=function(ctx: ChigiriQueryItemsContext, args: ChigiriQueryItemsArgs): ChigiriQueryItemsResult return {} end }\nreturn resolver",
    )
    .unwrap();
    assert!(
        directory
            .command(&["check", "--config-dir", "mock"])
            .status
            .success()
    );
    fs::create_dir(directory.0.join("mock/.chigiri-mock-transaction")).unwrap();
    for command in ["check", "serve", "update", "codegen", "export", "reference"] {
        let mut arguments = vec![command, "--config-dir", "mock"];
        if command == "export" {
            arguments.extend(["--output", "incomplete.toml"]);
        }
        let output = directory.command(&arguments);
        assert_eq!(output.status.code(), Some(1), "{command}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unfinished project transaction"));
    }
    assert!(!directory.0.join("incomplete.toml").exists());
}
