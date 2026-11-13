//! Verify closed diagnostics, exact locations, and confidentiality in the real binary.

use super::support::{Directory, Server, TEMPLATE, body_position, edited, with_seed, without};
use std::fs;

#[test]
fn mock_03_missing_duplicate_unknown_hierarchy_and_bounded_regular_utf8() {
    let _test_case = crate::support::test_case();
    let variants = [
        TEMPLATE.replace("format = \"chigiri-mock/v11\"", ""),
        format!("format = \"chigiri-mock/v11\"\n{TEMPLATE}"),
        TEMPLATE.replace("[server]", "[server]\nunknown = 1"),
        TEMPLATE.replace("[seed]", "[seed]\nunknown = 1"),
        TEMPLATE.replace("luau = '''", "unknown = 1\nluau = '''"),
        TEMPLATE.replace("luau = '''", "state = \"ready\"\nluau = '''"),
        TEMPLATE.replace("luau = '''", "state = { ready = [] }\nluau = '''"),
        TEMPLATE.replace(
            "[resolvers.\"Mutation.deletePost\"]",
            "[resolvers.\"Mutation.unknown\"]",
        ),
        without(TEMPLATE, "resolvers"),
        without(TEMPLATE, "seed"),
        edited("Query.getUsers", &" ".repeat(32 * 1024 + 1)),
        format!("{TEMPLATE}#{}", "x".repeat(256 * 1024)),
    ];
    for config in variants {
        let directory = Directory::new();
        fs::write(directory.0.join("mock.toml"), config).unwrap();
        let output = directory.command(&["serve", "--config", "mock.toml"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
    }
    let directory = Directory::new();
    fs::write(directory.0.join("invalid.toml"), [0xff, 0xfe]).unwrap();
    fs::write(directory.0.join("normal.toml"), TEMPLATE).unwrap();
    std::os::unix::fs::symlink("normal.toml", directory.0.join("link.toml")).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(directory.0.join("pipe.toml"))
            .status()
            .unwrap()
            .success()
    );
    for path in ["invalid.toml", "link.toml", "pipe.toml", "."] {
        assert_eq!(
            directory
                .command(&["serve", "--config", path])
                .status
                .code(),
            Some(1)
        );
    }
    let empty = with_seed(TEMPLATE, &serde_json::json!({"User":[],"Post":[]}));
    let mut server = Server::start(&empty);
    assert_eq!(
        server.graphql("{getUsers{id} findPost{id}}")["data"],
        serde_json::json!({"getUsers":[],"findPost":[]})
    );
    server.stop("-TERM");
}

#[test]
fn mock_14_type_compile_runtime_body_positions_and_private_text() {
    let _test_case = crate::support::test_case();
    for (body, phase) in [
        ("local private = \"DO_NOT_LEAK\"\nreturn 42", "type"),
        ("local private = \"DO_NOT_LEAK\"\nif then", "compile"),
    ] {
        let directory = Directory::new();
        let config = edited("Query.getUsers", body);
        let (line, column) = body_position(&config, "Query.getUsers");
        fs::write(directory.0.join("diagnostic.toml"), config).unwrap();
        let output = directory.command(&["serve", "--config", "diagnostic.toml"]);
        assert_eq!(output.status.code(), Some(1));
        let diagnostic = String::from_utf8(output.stderr).unwrap();
        let location = format!("diagnostic.toml:{line}:{column} Query.getUsers");
        assert!(diagnostic.contains(&location), "{diagnostic}");
        assert_eq!(diagnostic.contains(" Luau 5:"), phase == "type");
        assert!(diagnostic.contains(&format!("[{phase}]")), "{diagnostic}");
        assert!(!diagnostic.contains("DO_NOT_LEAK"));
        assert!(!diagnostic.contains(directory.0.to_str().unwrap()));
        if phase == "compile" {
            let checked =
                directory.command(&["check", "--config", "diagnostic.toml", "--format", "json"]);
            let report: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
            assert!(report["diagnostics"][0]["embedded"].is_null());
        }
        println!("{diagnostic}");
    }
    let directory = Directory::new();
    fs::write(
        directory.0.join("diagnostic.toml"),
        edited("Query.getUsers", "return 'wrong'"),
    )
    .unwrap();
    let checked = directory.command(&["check", "--config", "diagnostic.toml", "--format", "json"]);
    assert_eq!(checked.status.code(), Some(1));
    assert!(checked.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    let diagnostic = &report["diagnostics"][0];
    assert_eq!(diagnostic["member"], "Query.getUsers");
    assert_eq!(diagnostic["cause"], "MOCK_TYPE_MISMATCH");
    assert_eq!(diagnostic["expected"], "[User!]! result for Query.getUsers");
    assert_eq!(diagnostic["actual"], "string");
    assert!(
        !diagnostic
            .to_string()
            .contains(directory.0.to_str().unwrap())
    );

    fs::write(
        directory.0.join("diagnostic.toml"),
        edited("Query.getUsers", "return 1 == 2"),
    )
    .unwrap();
    let checked = directory.command(&["check", "--config", "diagnostic.toml", "--format", "json"]);
    assert_eq!(checked.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    let diagnostic = &report["diagnostics"][0];
    assert_eq!(diagnostic["cause"], "MOCK_CONTRACT");
    assert_eq!(
        diagnostic["expected"],
        "closed resolver module { resolve: generated function }"
    );
    assert_eq!(diagnostic["actual"], "structural type unavailable");
    assert_eq!(
        diagnostic["hint"],
        "Review the generated declarations and reported logical position."
    );

    let body = "local users = ctx.models.User:find({limit=args.limit})\nif args.limit == 1 then\n    error(\"DO_NOT_LEAK\")\nend\nreturn users";
    let config = edited("Query.getUsers", body);
    let mut server = Server::start(&config);
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &serde_json::json!({"query":"{getUsers(limit:1){id}}"}).to_string(),
    );
    assert_eq!(response.status, 500);
    let failed: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert!(failed["data"].is_null());
    assert_eq!(failed["errors"].as_array().unwrap().len(), 1);
    assert_eq!(failed["errors"][0]["message"], "Mock execution failed");
    assert!(!failed.to_string().contains("DO_NOT_LEAK"));
    assert!(
        !failed
            .to_string()
            .contains(server.directory.0.to_str().unwrap())
    );
    assert_eq!(
        server.graphql("{getUsers{id}}")["data"]["getUsers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    server.stop("-TERM");
    let stderr = fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
    let diagnostics: Vec<serde_json::Value> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|record| record["kind"] == "resolver_diagnostic")
        .collect();
    assert_eq!(diagnostics.len(), 1, "{stderr}");
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic["module"], "mock.toml");
    assert_eq!(diagnostic["coordinate"], "Query.getUsers");
    assert_eq!(diagnostic["phase"], "runtime");
    assert_eq!(diagnostic["cause"], "MOCK_CONTRACT");
    assert_eq!(diagnostic["line"], 6);
    assert_eq!(diagnostic["column"], 1);
    assert!(
        diagnostic["stack"].is_null(),
        "Pure exceptions must not expose a native stack."
    );
    assert_eq!(
        diagnostic["request_id"].as_u64().unwrap().to_string(),
        failed["errors"][0]["extensions"]["requestId"]
            .as_str()
            .unwrap()
    );
    assert!(!stderr.contains("DO_NOT_LEAK"));
    assert!(!stderr.contains(server.directory.0.to_str().unwrap()));
}

#[test]
fn resolver_and_hook_stacks_use_bounded_lines_and_stay_stderr_only() {
    let _case = crate::support::test_case();
    let resolver = edited(
        "Query.getUsers",
        r#"
if args.limit == 1 then
    local private = 'PRIVATE_STACK_VALUE'
    local bad:any = ctx.jsonObject
    bad(1)
end
return ctx.models.User:find({limit=args.limit})
"#,
    );
    let hook = super::hooks_http::with_hook(
        TEMPLATE,
        chigiri_mock::hooks::HookSlot::FieldBefore,
        r#"
if ctx.field.arguments.limit == 1 then
    local level:any = 'invalid'
    ctx.log(level, 'PRIVATE_STACK_VALUE')
end
return nil
"#,
    );
    let invalid = super::hooks_http::with_hook(
        TEMPLATE,
        chigiri_mock::hooks::HookSlot::FieldBefore,
        "if ctx.field.arguments.limit == 1 then ctx.reject({code=499}) end return nil",
    );
    for (config, kind) in [
        (resolver, "resolver_diagnostic"),
        (hook, "hook"),
        (invalid, "hook"),
    ] {
        let mut server = Server::start(&config);
        let response = server.http(
            "POST",
            &[("Content-Type", "application/json")],
            &serde_json::json!({"query":"{getUsers(limit:1){id}}"}).to_string(),
        );
        assert_eq!(response.status, 500);
        let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
        assert!(body["data"].is_null());
        assert!(!response.body.contains("stack"));
        assert!(!response.body.contains("PRIVATE_STACK_VALUE"));
        assert_eq!(
            server.graphql("{getUsers{id}}")["data"]["getUsers"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        server.stop("-TERM");
        let stderr = fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
        let records: Vec<serde_json::Value> = stderr
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|record| record["kind"] == kind)
            .collect();
        assert_eq!(records.len(), 1);
        let diagnostic = if kind == "hook" {
            &records[0]["diagnostic"]
        } else {
            &records[0]
        };
        assert_eq!(diagnostic["module"], "mock.toml");
        assert_eq!(diagnostic["cause"], "MOCK_CONTRACT");
        let stack = &diagnostic["stack"];
        assert_eq!(stack["origin"], "host");
        let lines = stack["lines"].as_array().unwrap();
        assert!(!lines.is_empty() && lines.len() <= 16);
        assert!(lines.iter().all(|line| line.as_u64().unwrap() > 0));
        assert!(!stderr.contains("PRIVATE_STACK_VALUE"));
        assert!(!stderr.contains(server.directory.0.to_str().unwrap()));
    }
}
