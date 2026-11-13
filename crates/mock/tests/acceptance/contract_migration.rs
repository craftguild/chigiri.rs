//! Prove fresh-destination migration copies only user-owned Mock material.

use std::fs;

use serde_json::json;

use crate::support::{Directory, Server};

const PUBLIC_SDL: &str = r#"
type Todo { id: ID! title: String! }
type Query { todos: [Todo!]! }
"#;
const STORAGE_SDL: &str = "extend type Todo { internalTag: String! }\n";
const STUB_BODY: &str = r#"        ctx.fail({
            code = "NOT_IMPLEMENTED",
            message = "This operation is not implemented.",
        })"#;
const HOOK_BODY: &str = r#"        -- Leave request state unchanged for this phase.
        return nil"#;

#[test]
fn cv_06_fresh_destination_migrates_user_sources_without_old_generated_metadata() {
    let _case = crate::support::test_case();
    let directory = Directory::new();

    let legacy = directory.0.join("legacy-user-material");
    fs::create_dir(&legacy).unwrap();
    fs::write(legacy.join("public.graphql"), PUBLIC_SDL).unwrap();
    fs::write(legacy.join("storage.graphql"), STORAGE_SDL).unwrap();
    fs::write(
        legacy.join("resolver-body.luau"),
        "return ctx.models.Todo:find({})",
    )
    .unwrap();
    fs::write(
        legacy.join("hook-body.luau"),
        "return { statePatch = { migrated = true } }",
    )
    .unwrap();
    fs::write(
        legacy.join("Todo.jsonl"),
        "{\"id\":\"todo-1\",\"title\":\"Migrated\",\"internalTag\":\"private\"}\n",
    )
    .unwrap();

    let initialized = directory.command(&[
        "init",
        "--schema",
        "legacy-user-material/public.graphql",
        "--config-dir",
        "mock",
    ]);
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let fresh = directory.0.join("mock");
    fs::copy(
        legacy.join("storage.graphql"),
        fresh.join("storage.graphql"),
    )
    .unwrap();
    let config_path = fresh.join("mock.luau");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("127.0.0.1:4000", "127.0.0.1:0")
        .replace("request = { before = false", "request = { before = true")
        .replace(
            "state = { fields = {}, initial = {} }",
            "state = {\n        fields = { migrated = { kind = \"boolean\" } },\n        initial = { migrated = false },\n    }",
        );
    fs::write(&config_path, config).unwrap();
    let updated = directory.command(&["update", "--config-dir", "mock"]);
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );

    let resolver_path = fresh.join("resolvers/query/todos.luau");
    let resolver = fs::read_to_string(&resolver_path).unwrap();
    let resolver_body = fs::read_to_string(legacy.join("resolver-body.luau")).unwrap();
    fs::write(
        &resolver_path,
        resolver.replace(STUB_BODY, &format!("        {resolver_body}")),
    )
    .unwrap();
    let hook_path = fresh.join("hooks/request/before.luau");
    let hook = fs::read_to_string(&hook_path).unwrap();
    let hook_body = fs::read_to_string(legacy.join("hook-body.luau")).unwrap();
    fs::write(
        &hook_path,
        hook.replace(HOOK_BODY, &format!("        {hook_body}")),
    )
    .unwrap();
    fs::copy(legacy.join("Todo.jsonl"), fresh.join("seeds/Todo.jsonl")).unwrap();

    let adapted_resolver = fs::read(&resolver_path).unwrap();
    let adapted_hook = fs::read(&hook_path).unwrap();
    let adapted_seed = fs::read(fresh.join("seeds/Todo.jsonl")).unwrap();
    assert!(
        directory
            .command(&["update", "--config-dir", "mock"])
            .status
            .success()
    );
    assert_eq!(fs::read(&resolver_path).unwrap(), adapted_resolver);
    assert_eq!(fs::read(&hook_path).unwrap(), adapted_hook);
    assert_eq!(
        fs::read(fresh.join("seeds/Todo.jsonl")).unwrap(),
        adapted_seed
    );
    assert!(
        directory
            .command(&["check", "--config-dir", "mock"])
            .status
            .success()
    );
    assert!(
        directory
            .command(&["export", "--config-dir", "mock", "--output", "fresh.toml",])
            .status
            .success()
    );
    let bundle = fs::read_to_string(directory.0.join("fresh.toml")).unwrap();
    assert!(bundle.contains("chigiri-mock/v11"));
    assert!(bundle.contains("migrated"));
    assert!(!bundle.contains("chigiri-mock-project/v6"));

    let mut server = Server::project(directory);
    assert_eq!(
        server.graphql("{todos{id title}}")["data"]["todos"],
        json!([{"id":"todo-1","title":"Migrated"}])
    );
    server.stop("-TERM");
}
