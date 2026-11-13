//! Verify explicit GraphiQL exposure, real bind origins, non-persistent setup, and readiness in one binary.

use serde_json::json;
use std::fs;

use super::support::{Directory, Server, TEMPLATE, edited, request_path};

const SCHEMA_QUERY: &str = "{__schema{queryType{name} types{name}}}";

#[test]
fn dx35_19_20_22_mock_opt_in_uses_numeric_ipv4_ipv6_port_zero_and_exact_origins() {
    let _case = crate::support::test_case();
    for ipv6 in [false, true] {
        for enabled in [false, true] {
            let config = if ipv6 {
                TEMPLATE.replace("127.0.0.1:4000", "[::1]:0")
            } else {
                TEMPLATE.into()
            };
            let mut server = if enabled {
                Server::with_graphiql(&config, false)
            } else {
                Server::start(&config)
            };
            assert_eq!(server.address.is_ipv6(), ipv6);
            let before = fs::read(server.directory.0.join("mock.toml")).unwrap();
            let own = format!("http://{}", server.address);
            let document = json!({"query": SCHEMA_QUERY}).to_string();
            let response = server.http("POST", &[("Content-Type", "application/json")], &document);
            assert_eq!(response.status, if enabled { 200 } else { 500 });
            let result: serde_json::Value = serde_json::from_str(&response.body).unwrap();
            assert_eq!(result.get("errors").is_none(), enabled);
            if !enabled {
                assert!(result["data"].is_null());
                assert_eq!(result["errors"].as_array().unwrap().len(), 1);
                assert_eq!(
                    result["errors"][0]["extensions"]["code"],
                    "MOCK_EXECUTION_FAILED"
                );
            }
            for origin in [own.as_str(), "http://localhost:5173"] {
                let response = server.http(
                    "POST",
                    &[
                        ("Content-Type", "application/json"),
                        ("Origin", origin),
                        ("Host", "forged.invalid"),
                    ],
                    &document,
                );
                let allowed = enabled || origin != own;
                assert_eq!(
                    response.status,
                    if !allowed {
                        403
                    } else if enabled {
                        200
                    } else {
                        500
                    }
                );
                assert_eq!(
                    response
                        .headers
                        .contains(&format!("access-control-allow-origin: {origin}")),
                    allowed
                );
                assert!(
                    !response
                        .headers
                        .contains("access-control-allow-credentials")
                );
            }
            for origin in [
                "null".to_owned(),
                "".into(),
                "http://forged.invalid".into(),
                format!("http://localhost:{}", server.address.port()),
                "http://127.0.0.1:0".into(),
            ] {
                assert_eq!(
                    server
                        .http(
                            "POST",
                            &[("Origin", &origin), ("Host", "forged.invalid")],
                            "{}"
                        )
                        .status,
                    403
                );
            }
            assert_eq!(
                server
                    .http("POST", &[("Origin", &own), ("Origin", &own)], "{}")
                    .status,
                403
            );
            for path in [
                "/graphiql",
                "/graphiql/assets/app.js",
                "/graphiql/assets/app.css",
                "/graphiql/assets/worker-graphql.js",
            ] {
                let get = request_path(server.address, "GET", path, &[], "");
                assert_eq!(get.status, if enabled { 200 } else { 404 });
                let head = request_path(server.address, "HEAD", path, &[], "");
                assert_eq!(head.status, get.status);
                assert!(head.body.is_empty());
                assert_eq!(
                    request_path(server.address, "POST", path, &[], "{}").status,
                    if enabled { 405 } else { 404 }
                );
                if enabled {
                    assert!(get.headers.contains("x-content-type-options: nosniff"));
                    assert!(get.headers.contains("content-security-policy:"));
                }
            }
            assert_eq!(
                request_path(
                    server.address,
                    "GET",
                    "/graphiql/assets/missing.js",
                    &[],
                    ""
                )
                .status,
                404
            );
            let output = fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
            assert!(
                output.contains(&format!("listening {own}/graphql")),
                "{own}: {output}"
            );
            assert_eq!(
                output.contains(&format!("GraphiQL {own}/graphiql")),
                enabled,
                "{own}: {output}"
            );
            assert_eq!(
                output.contains("schema introspection is public"),
                enabled,
                "{own}: {output}"
            );
            assert_eq!(
                server.graphql("{ getUsers { id posts { id author { id } } } }")["data"]["getUsers"]
                    [0]["posts"][0]["author"]["id"],
                "User-1"
            );
            server.stop("-TERM");
            assert_eq!(
                fs::read(server.directory.0.join("mock.toml")).unwrap(),
                before
            );
        }
    }
}

#[test]
fn dx35_19_20_project_and_portable_serve_share_ui_without_persisting_the_flag() {
    let _case = crate::support::test_case();
    let directory =
        crate::project_workflow::project(include_str!("../../fixtures/blog.graphql"), TEMPLATE);
    let settings = directory.0.join("mock/mock.luau");
    let original = fs::read_to_string(&settings).unwrap();
    fs::write(&settings, original.replace("127.0.0.1:4000", "127.0.0.1:0")).unwrap();
    let before = fs::read(&settings).unwrap();
    assert!(
        directory
            .command(&["export", "--config-dir", "mock", "--output", "before.toml"])
            .status
            .success()
    );
    let mut project = Server::project_with_graphiql(directory);
    assert!(project.graphql(SCHEMA_QUERY).get("errors").is_none());
    assert_eq!(
        request_path(project.address, "GET", "/graphiql", &[], "").status,
        200
    );
    project.stop("-INT");
    assert_eq!(fs::read(&settings).unwrap(), before);
    assert!(
        project
            .directory
            .command(&["export", "--config-dir", "mock", "--output", "after.toml"])
            .status
            .success()
    );
    let bundle = fs::read(project.directory.0.join("before.toml")).unwrap();
    assert_eq!(
        fs::read(project.directory.0.join("after.toml")).unwrap(),
        bundle
    );
    assert!(!String::from_utf8_lossy(&bundle).contains("graphiql"));
    let mut portable = Server::with_graphiql(std::str::from_utf8(&bundle).unwrap(), true);
    assert_eq!(
        request_path(portable.address, "GET", "/graphiql/assets/app.js", &[], "").status,
        200
    );
    assert!(portable.graphql(SCHEMA_QUERY).get("errors").is_none());
    portable.stop("-TERM");
}

#[test]
fn dx35_19_20_graphiql_flag_does_not_skip_readiness_or_write_on_help_and_usage() {
    let _case = crate::support::test_case();
    let directory = Directory::new();
    let help = directory.command(&["serve", "--config", "absent", "--enable-graphiql", "--help"]);
    assert!(help.status.success());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("schema introspection")
    );
    for arguments in [
        vec!["check", "--enable-graphiql", "--config", "missing"],
        vec![
            "serve",
            "--config",
            "missing",
            "--enable-graphiql",
            "--enable-graphiql",
        ],
        vec![
            "serve",
            "--config",
            "missing",
            "--config-dir",
            "missing",
            "--enable-graphiql",
        ],
    ] {
        assert_eq!(directory.command(&arguments).status.code(), Some(2));
    }
    let mut invalid_seed = super::support::seed(TEMPLATE);
    invalid_seed["Post"][0]["authorId"] = json!("missing");
    for config in [
        TEMPLATE.replace("127.0.0.1:4000", "0.0.0.0:0"),
        super::support::with_seed(TEMPLATE, &invalid_seed),
        edited("Mutation.createUser", "return \"wrong type\""),
    ] {
        fs::write(directory.0.join("invalid.toml"), &config).unwrap();
        let result = directory.command(&["serve", "--config", "invalid.toml", "--enable-graphiql"]);
        assert_eq!(
            result.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty());
        assert_eq!(
            fs::read_to_string(directory.0.join("invalid.toml")).unwrap(),
            config
        );
    }
}

#[test]
fn dx35_20_22_static_ui_bypasses_worker_capacity_while_graphql_preserves_runtime_guards() {
    let _case = crate::support::test_case();
    let body = "local user = ctx.models.User:create(args.input) if args.input.name == \"wait\" then while true do end end return user";
    let mut server = Server::with_graphiql(
        &crate::budgets::slow_config(&edited("Mutation.createUser", body)),
        false,
    );
    let connection = crate::budgets::begin_slow(&server);
    crate::budgets::assert_waiting(&connection);
    for path in ["/graphiql", "/graphiql/assets/app.js"] {
        assert_eq!(
            request_path(server.address, "GET", path, &[], "").status,
            200
        );
    }
    crate::budgets::assert_waiting(&connection);
    drop(connection);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
    loop {
        let response = server.http(
            "POST",
            &[("Content-Type", "application/json")],
            r#"{"query":"{getUsers{id}}"}"#,
        );
        if response.status == 200 {
            let result: serde_json::Value = serde_json::from_str(&response.body).unwrap();
            assert_eq!(
                result["data"]["getUsers"],
                json!([{"id":"User-1"},{"id":"User-2"}])
            );
            break;
        }
        assert_eq!(response.status, 503);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        std::time::Instant::now() < deadline,
        "worker capacity should recover before the VM deadline"
    );
    assert_eq!(
        server.graphql("mutation{createUser(input:{name:\"ok\"}){id}}")["data"]["createUser"]["id"],
        "User-3"
    );
    assert_eq!(
        server
            .http(
                "POST",
                &[("Content-Type", "application/json")],
                &"x".repeat(1024 * 1024 + 1)
            )
            .status,
        413
    );
    let nested = format!(
        "{{getUsers{{{}id{}}}}}",
        "posts{author{".repeat(20),
        "}}".repeat(20)
    );
    assert_eq!(
        server
            .http(
                "POST",
                &[("Content-Type", "application/json")],
                &json!({"query": nested}).to_string()
            )
            .status,
        422
    );
    let connection = crate::budgets::begin_slow(&server);
    crate::budgets::assert_waiting(&connection);
    server.stop("-TERM");
    drop(connection);
}
