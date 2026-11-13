//! Prove scaffold admission, intentional failure semantics, coexistence, rollback, and recovery.

use std::fs;

use serde_json::{Value, json};

use crate::support::{Directory, Server};

const SDL: &str = r#"
type Todo { id: ID! title: String! }
type Query {
  todos: [Todo!]!
  health: String!
  broken: String!
}
type Mutation {
  addTodo(title: String!): Todo!
  notReady: Boolean!
}
"#;
const INTENTIONAL_ERROR: &str = r#"        ctx.fail({
            code = "NOT_IMPLEMENTED",
            message = "This operation is not implemented.",
        })"#;

fn project() -> Directory {
    let directory = Directory::new();
    fs::write(directory.0.join("todo.graphql"), SDL).unwrap();
    let initialized =
        directory.command(&["init", "--schema", "todo.graphql", "--config-dir", "mock"]);
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let config = directory.0.join("mock/mock.luau");
    let source = fs::read_to_string(&config).unwrap();
    fs::write(config, source.replace("127.0.0.1:4000", "127.0.0.1:0")).unwrap();
    directory
}

fn check(directory: &Directory) {
    let checked = directory.command(&["check", "--config-dir", "mock"]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
}

fn implement(directory: &Directory, relative: &str, body: &str) {
    let path = directory.0.join("mock/resolvers").join(relative);
    let source = fs::read_to_string(&path).unwrap();
    assert_eq!(source.matches(INTENTIONAL_ERROR).count(), 1, "{relative}");
    let body = format!("        {}", body.replace('\n', "\n        "));
    fs::write(path, source.replace(INTENTIONAL_ERROR, &body)).unwrap();
}

fn response(server: &Server, query: &str) -> (u16, Value) {
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query": query}).to_string(),
    );
    (
        response.status,
        serde_json::from_str(&response.body).unwrap(),
    )
}

fn assert_not_implemented(status: u16, value: &Value, path: &str) {
    assert_eq!(status, 200);
    assert!(value["data"].is_null());
    let error = &value["errors"][0];
    assert_eq!(error["message"], "This operation is not implemented.");
    assert_eq!(error["path"], json!([path]));
    assert_eq!(error["extensions"]["code"], "NOT_IMPLEMENTED");
    assert!(
        error["extensions"]["requestId"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
}

#[test]
fn cv_01_02_03_scaffolds_admit_and_fail_intentionally_without_weakening_atomicity() {
    let _case = crate::support::test_case();

    let scaffold = project();
    check(&scaffold);
    let exported = scaffold.command(&[
        "export",
        "--config-dir",
        "mock",
        "--output",
        "scaffold.toml",
    ]);
    assert!(exported.status.success());
    let bundle = fs::read_to_string(scaffold.0.join("scaffold.toml")).unwrap();

    let mut project_server = Server::project(scaffold);
    let (status, value) = response(&project_server, "{todos{id}}");
    assert_not_implemented(status, &value, "todos");
    project_server.stop("-TERM");

    let mut bundle_server = Server::start(&bundle);
    let (status, value) = response(&bundle_server, "{todos{id}}");
    assert_not_implemented(status, &value, "todos");
    bundle_server.stop("-TERM");

    let partial = project();
    implement(
        &partial,
        "query/todos.luau",
        "return ctx.models.Todo:find({})",
    );
    implement(&partial, "query/health.luau", "return 'ok'");
    implement(
        &partial,
        "query/broken.luau",
        "error('PRIVATE_EXECUTION_DETAIL')",
    );
    implement(
        &partial,
        "mutation/addTodo.luau",
        "return ctx.models.Todo:create({title=args.title})",
    );
    check(&partial);

    let mut server = Server::project(partial);
    assert_eq!(server.graphql("{health}")["data"], json!({"health":"ok"}));
    let (status, value) = response(&server, "mutation{notReady}");
    assert_not_implemented(status, &value, "notReady");
    assert_eq!(server.graphql("{health}")["data"], json!({"health":"ok"}));

    let (status, failed) = response(
        &server,
        "mutation{first:addTodo(title:\"one\"){id} second:notReady}",
    );
    assert_not_implemented(status, &failed, "second");
    assert_eq!(server.graphql("{todos{id}}")["data"], json!({"todos":[]}));

    let created = server.graphql("mutation{addTodo(title:\"saved\"){title}}");
    assert_eq!(created["data"]["addTodo"]["title"], "saved");
    assert_eq!(server.graphql("{health}")["data"], json!({"health":"ok"}));

    let (status, internal) = response(&server, "{broken}");
    assert_eq!(status, 500);
    assert_eq!(internal["errors"][0]["message"], "Mock execution failed");
    assert_eq!(
        internal["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    assert!(!internal.to_string().contains("PRIVATE_EXECUTION_DETAIL"));
    server.stop("-TERM");
}

#[test]
fn cv_r51_01_failure_type_alias_does_not_change_cli_or_http_contracts() {
    let _case = crate::support::test_case();

    let invalid = project();
    implement(
        &invalid,
        "query/health.luau",
        "type ChigiriBusinessFailure = any\nctx.fail({code=7,message=42})",
    );
    let rejected = invalid.command(&[
        "check",
        "--config-dir",
        "mock",
        "--target",
        "resolvers/query/health.luau",
        "--format",
        "json",
    ]);
    assert_eq!(rejected.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(report["valid"], false);
    assert_eq!(report["diagnostics"][0]["phase"], "type");

    let valid = project();
    implement(
        &valid,
        "query/health.luau",
        "type ChigiriBusinessFailure = number\n\
         ctx.fail({code='DENIED',message='Access denied'})",
    );
    check(&valid);
    let mut server = Server::project(valid);
    let (status, value) = response(&server, "{health}");
    assert_eq!(status, 200);
    assert!(value["data"].is_null());
    assert_eq!(value["errors"][0]["message"], "Access denied");
    assert_eq!(value["errors"][0]["extensions"]["code"], "DENIED");
    server.stop("-TERM");
}

#[test]
fn cv_r51_02_workflow_example_matches_its_generated_contract() {
    let _case = crate::support::test_case();
    let directory = Directory::new();
    fs::write(
        directory.0.join("notes.graphql"),
        "type Note { id: ID! title: String! } type Query { notes: [Note!]! }",
    )
    .unwrap();
    let initialized =
        directory.command(&["init", "--schema", "notes.graphql", "--config-dir", "mock"]);
    assert!(initialized.status.success());

    let guide = include_str!("../../../../docs/mock-project-workflow.md");
    let section = guide.split_once("a completed example is:").unwrap().1;
    let example = section
        .split_once("```luau\n")
        .unwrap()
        .1
        .split_once("\n```")
        .unwrap()
        .0;
    fs::write(directory.0.join("mock/resolvers/query/notes.luau"), example).unwrap();
    check(&directory);
}
