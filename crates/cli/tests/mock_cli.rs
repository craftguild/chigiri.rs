//! Exercise MOCK-02 through the real CLI and MOCK-14/15 through real HTTP.

#[path = "../../mock/tests/acceptance/admission.rs"]
mod admission;
#[path = "../../mock/tests/acceptance/adopted_migration.rs"]
mod adopted_migration;
#[path = "../../mock/tests/acceptance/agent_ready.rs"]
mod agent_ready;
#[path = "../../mock/tests/acceptance/authoring_second_domain.rs"]
mod authoring_second_domain;
#[path = "../../mock/tests/acceptance/budgets.rs"]
mod budgets;
#[path = "../../mock/tests/acceptance/contract_migration.rs"]
mod contract_migration;
#[path = "../../mock/tests/acceptance/contract_validation.rs"]
mod contract_validation;
#[path = "../../mock/tests/acceptance/diagnostic_navigation.rs"]
mod diagnostic_navigation;
#[path = "../../mock/tests/acceptance/diagnostics.rs"]
mod diagnostics;
#[path = "../../mock/tests/acceptance/graphiql.rs"]
mod graphiql;
#[path = "../../mock/tests/acceptance/hook_resources.rs"]
mod hook_resources;
#[path = "../../mock/tests/acceptance/hooks_http.rs"]
mod hooks_http;
#[path = "../../mock/tests/acceptance/host_boundary.rs"]
mod host_boundary;
#[path = "../../mock/tests/acceptance/inputs.rs"]
mod inputs;
#[path = "../../mock/tests/acceptance/logging.rs"]
mod logging;
#[path = "../../mock/tests/acceptance/logging_shutdown.rs"]
mod logging_shutdown;
#[path = "../../mock/tests/acceptance/mixed_identity.rs"]
mod mixed_identity;
#[path = "../../mock/tests/acceptance/output_limits.rs"]
mod output_limits;
#[path = "../../mock/tests/acceptance/project_equivalence.rs"]
mod project_equivalence;
#[path = "../../mock/tests/acceptance/project_update.rs"]
mod project_update;
#[path = "../../mock/tests/acceptance/project_workflow.rs"]
mod project_workflow;
#[path = "../../mock/tests/acceptance/reentry.rs"]
mod reentry;
#[path = "../../mock/tests/acceptance/runtime_schema/mod.rs"]
mod runtime_schema;
#[path = "../../mock/tests/acceptance/schema_sync_cli.rs"]
mod schema_sync_cli;
#[path = "../../mock/tests/acceptance/semantics.rs"]
mod semantics;
#[path = "../../mock/tests/acceptance/soft_limits.rs"]
mod soft_limits;
#[path = "../../mock/tests/acceptance/stateful_domain.rs"]
mod stateful_domain;
#[path = "../../mock/tests/acceptance/stateful_domain_fixture.rs"]
mod stateful_domain_fixture;
#[path = "../../mock/tests/acceptance/stateful_domain_support.rs"]
mod stateful_domain_support;
#[path = "../../mock/tests/acceptance/subscriptions/mod.rs"]
mod subscriptions;
#[path = "../../mock/tests/support/mod.rs"]
mod support;
#[path = "../../mock/tests/acceptance/terminal.rs"]
mod terminal;
#[path = "../../mock/tests/acceptance/timeleap_http.rs"]
mod timeleap_http;
#[path = "../../mock/tests/acceptance/transactions.rs"]
mod transactions;
#[path = "../../mock/tests/acceptance/transport.rs"]
mod transport;
use serde_json::json;
use std::fs;
use support::{Directory, Server, TEMPLATE, edited, seed, with_seed};

#[test]
fn mock_02_offline_help_version_and_deterministic_exclusive_init() {
    let _test_case = crate::support::test_case();
    let directory = Directory::new();
    fs::write(
        directory.0.join("schema.graphql"),
        include_str!("../../mock/fixtures/blog.graphql"),
    )
    .unwrap();
    for flag in ["--help", "--version"] {
        let result = directory.command(&[flag]);
        assert!(result.status.success());
        let stdout = String::from_utf8(result.stdout).unwrap();
        assert!(stdout.contains("chigiri mock"));
        assert!(!stdout.contains("list-unimplemented"));
        if flag == "--version" {
            assert!(stdout.starts_with(include_str!("../assets/version-banner.txt")));
            assert!(stdout.contains("\nluaur: 0.1.8\n"));
        }
    }
    let bare = directory.command(&[]);
    assert!(bare.status.success());
    assert!(
        String::from_utf8(bare.stdout)
            .unwrap()
            .contains("chigiri mock")
    );
    for project in ["first", "second"] {
        let output = directory.command(&[
            "init",
            "--schema",
            "schema.graphql",
            "--config-dir",
            project,
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("validation_checked = false"));
        let guidance = fs::read_to_string(directory.0.join(project).join("AGENTS.md")).unwrap();
        assert!(guidance.contains("chigiri mock check --config-dir ."));
        assert!(guidance.contains("Do not edit"));
        let path = format!("{project}.toml");
        assert!(
            directory
                .command(&["check", "--config-dir", project])
                .status
                .success()
        );
        assert!(
            directory
                .command(&["export", "--config-dir", project, "--output", &path])
                .status
                .success()
        );
        assert!(directory.0.join(path).is_file());
    }

    assert_eq!(
        directory
            .command(&[
                "init",
                "--schema",
                "schema.graphql",
                "--config-dir",
                "first"
            ])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        directory
            .command(&["init", "--schema", "schema.graphql", "--config-dir", "."])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        directory
            .command(&[
                "init",
                "--schema",
                "schema.graphql",
                "--config-dir",
                "missing/new-project"
            ])
            .status
            .code(),
        Some(2)
    );
    std::os::unix::fs::symlink("absent", directory.0.join("link.toml")).unwrap();
    assert_eq!(
        directory
            .command(&[
                "init",
                "--schema",
                "schema.graphql",
                "--config-dir",
                "link.toml"
            ])
            .status
            .code(),
        Some(1)
    );
    for arguments in [
        vec!["serve"],
        vec!["init"],
        vec!["init", "--out", "unused.toml"],
        vec!["--version", "--config", "missing"],
        vec!["serve", "--config", "x", "--listen", "127.0.0.1:0"],
        vec!["check", "--list-unimplemented", "--config-dir", "first"],
    ] {
        assert_eq!(directory.command(&arguments).status.code(), Some(2));
    }
}

#[test]
fn mock_03_rejects_closed_config_seed_and_all_script_errors_before_listen() {
    let _test_case = crate::support::test_case();
    let mut unknown = seed(TEMPLATE);
    unknown["User"][0]["extra"] = json!(1);
    let mut missing = seed(TEMPLATE);
    missing["Post"][0]["authorId"] = json!("missing");
    let variants = [
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v0"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v1"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v2"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v3"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v6"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v7"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v8"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v9"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v10"),
        TEMPLATE.replace(
            "chigiri-mock-capabilities/v4",
            "chigiri-mock-capabilities/v3",
        ),
        format!("unknown = true\n{TEMPLATE}"),
        TEMPLATE.replace("allowed_origins =", "unexpected = true\nallowed_origins ="),
        with_seed(TEMPLATE, &unknown),
        TEMPLATE.replace("User-2", "User-1"),
        with_seed(TEMPLATE, &missing),
        TEMPLATE.replace("127.0.0.1:4000", "0.0.0.0:0"),
        TEMPLATE.replace("127.0.0.1:4000", "localhost:4000"),
        TEMPLATE.replace("http://localhost:5173", "*"),
        edited("Mutation.deletePost", "return \"wrong\""),
        edited(
            "Query.getUsers",
            "return ctx.models.User:find({ limit = \"wrong\" })",
        ),
        edited("Mutation.updatePost", "if then"),
    ];
    for config in variants {
        assert_ne!(
            config, TEMPLATE,
            "the invalid fixture must change the input"
        );
        let directory = Directory::new();
        fs::write(directory.0.join("mock.toml"), config).unwrap();
        let result = directory.command(&["serve", "--config", "mock.toml"]);
        assert_eq!(
            result.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("mock.toml"));
    }
}

#[test]
fn mock_04_real_http_all_six_roots_and_relation_changes() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    assert_eq!(
        server.graphql("{ getUsers { id name } }")["data"]["getUsers"],
        json!([
            {"id":"User-1","name":"Alice"}, {"id":"User-2","name":"Bob"}
        ])
    );
    let user = server.graphql("mutation { createUser(input: {name: \"Carol\"}) { id name } }");
    assert_eq!(
        user["data"]["createUser"],
        json!({"id":"User-3","name":"Carol"})
    );
    let created = server.graphql("mutation { createPost(input: {title: \"new\", body: \"body\", authorId: \"User-1\"}) { id title author { id } } }");
    assert_eq!(
        created["data"]["createPost"],
        json!({"id":"Post-2","title":"new","author":{"id":"User-1"}})
    );
    let changed = server.graphql("mutation { updatePost(id: \"Post-2\", input: {authorId: \"User-2\"}) { id title body author { id posts { id } } } }");
    assert_eq!(
        changed["data"]["updatePost"],
        json!({"id":"Post-2","title":"new","body":"body","author":{"id":"User-2","posts":[{"id":"Post-2"}]}})
    );
    assert_eq!(
        server.graphql("{ findPost(authorId: \"User-2\") { id } }")["data"]["findPost"],
        json!([{"id":"Post-2"}])
    );
    let removed = server.graphql("mutation { deletePost(id: \"Post-2\") }");
    assert_eq!(removed["data"]["deletePost"], true);
    assert_eq!(
        server.graphql("{ findPost(id: \"Post-2\") { id } }")["data"]["findPost"],
        json!([])
    );
    let response = server.http(
        "POST",
        &[
            ("Content-Type", "application/json"),
            ("Origin", "http://localhost:5173"),
        ],
        "{\"query\":\"{ getUsers { id } }\"}",
    );
    assert_eq!(response.status, 200);
    assert!(
        response
            .headers
            .contains("access-control-allow-origin: http://localhost:5173")
    );
    assert!(
        !response
            .headers
            .contains("access-control-allow-credentials")
    );
    server.stop("-TERM");
}

#[test]
fn mock_14_edited_toml_changes_runtime_behavior() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(&edited(
        "Query.getUsers",
        "return ctx.models.User:find({ limit = 1 })",
    ));
    assert_eq!(
        server.graphql("{ getUsers(limit: 100) { id } }")["data"]["getUsers"],
        json!([{"id":"User-1"}])
    );
    server.stop("-INT");
}
