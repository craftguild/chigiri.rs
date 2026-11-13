//! External agents can query focused contracts and validate incremental edits.

use chigiri_mock::{
    hooks::HookSlot,
    luau::render_hook_module,
    project::{Layout, read_project_schema, update_project},
};
use serde_json::Value;
use std::{
    fs,
    process::{Command, Stdio},
};

use crate::{
    project_workflow::project,
    support::{BINARY, Directory, TEMPLATE},
};

#[test]
fn ai_ready_target_check_is_incremental_and_json_is_closed() {
    let _test_case = crate::support::test_case();
    let directory = project(include_str!("../../fixtures/blog.graphql"), TEMPLATE);
    let root = directory.0.join("mock");
    let schema = read_project_schema(&root).unwrap();
    let layout = Layout::new(schema.ir()).unwrap();
    assert!(layout.resolvers.len() > 1);
    let selected = &layout.resolvers[0].path;
    let unrelated = &layout.resolvers[1].path;
    let original = fs::read_to_string(root.join(unrelated.as_str())).unwrap();
    fs::write(root.join(unrelated.as_str()), "return {").unwrap();

    let partial = directory.command(&[
        "check",
        "--config-dir",
        "mock",
        "--target",
        selected,
        "--format",
        "json",
    ]);
    assert!(
        partial.status.success(),
        "{}",
        String::from_utf8_lossy(&partial.stderr)
    );
    assert!(partial.stderr.is_empty());
    let partial: Value = serde_json::from_slice(&partial.stdout).unwrap();
    assert_eq!(partial["format"], "chigiri-mock-check/v3");
    assert_eq!(partial["scope"], "targets");
    assert_eq!(partial["valid"], true);
    assert_eq!(partial["readiness_checked"], false);
    assert_eq!(partial["targets"], serde_json::json!([selected]));
    assert_eq!(partial["diagnostics"], serde_json::json!([]));

    let full = directory.command(&["check", "--config-dir", "mock", "--format", "json"]);
    assert_eq!(full.status.code(), Some(1));
    assert!(full.stderr.is_empty());
    let full: Value = serde_json::from_slice(&full.stdout).unwrap();
    assert_eq!(full["scope"], "project");
    assert_eq!(full["valid"], false);
    assert_eq!(full["readiness_checked"], false);
    assert_eq!(full["diagnostics"].as_array().unwrap().len(), 1);
    let diagnostic = &full["diagnostics"][0];
    assert!(diagnostic["member"].is_null());
    assert_eq!(diagnostic["cause"], "MOCK_LUAU_SYNTAX");
    assert_eq!(diagnostic["expected"], "valid Luau module");
    assert_eq!(diagnostic["actual"], "syntax error");
    assert_eq!(
        diagnostic["hint"],
        "Fix the syntax at the reported logical position."
    );
    assert_eq!(
        diagnostic.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec![
            "actual",
            "cause",
            "column",
            "coordinate",
            "embedded",
            "expected",
            "file",
            "hint",
            "line",
            "member",
            "message",
            "phase",
            "references",
            "severity",
        ]
    );
    let references = diagnostic["references"].as_array().unwrap();
    assert!(references.iter().any(|reference| {
        reference["topic"] == "resolvers" && reference["name"] == diagnostic["coordinate"]
    }));
    for reference in references {
        let topic = reference["topic"].as_str().unwrap();
        let name = reference["name"].as_str().unwrap();
        let resolved = directory.command(&[
            "reference",
            topic,
            name,
            "--config-dir",
            "mock",
            "--format",
            "json",
        ]);
        assert!(resolved.status.success(), "{topic} {name}");
    }
    let text = directory.command(&["check", "--config-dir", "mock"]);
    assert_eq!(text.status.code(), Some(1));
    let text = String::from_utf8(text.stderr).unwrap();
    for reference in references {
        assert!(text.contains(&format!(
            "chigiri mock reference {} {} --config-dir <PROJECT>",
            reference["topic"].as_str().unwrap(),
            reference["name"].as_str().unwrap()
        )));
    }
    assert!(text.contains("replace <PROJECT>"));
    assert!(!text.contains(root.to_str().unwrap()));
    assert!(!diagnostic.to_string().contains(root.to_str().unwrap()));

    for targets in [
        vec!["generated/types.luau"],
        vec!["../schema.graphql"],
        vec!["/tmp/resolver.luau"],
        vec!["resolvers/**/*.luau"],
        vec![selected, selected],
    ] {
        let mut arguments = vec!["check", "--config-dir", "mock", "--format", "json"];
        for target in targets {
            arguments.extend(["--target", target]);
        }
        let rejected = directory.command(&arguments);
        assert_eq!(rejected.status.code(), Some(1));
        assert!(rejected.stderr.is_empty());
        let rejected: Value = serde_json::from_slice(&rejected.stdout).unwrap();
        assert_eq!(rejected["valid"], false);
        assert_eq!(rejected["targets"], serde_json::json!([]));
        assert_eq!(rejected["diagnostics"][0]["phase"], "ownership");
        assert!(!rejected.to_string().contains("/tmp/resolver.luau"));
    }

    fs::write(root.join(unrelated.as_str()), &original).unwrap();
    let target_ready = directory.command(&[
        "check",
        "--config-dir",
        "mock",
        "--target",
        selected,
        "--format",
        "json",
    ]);
    assert!(target_ready.status.success());
    let target_ready: Value = serde_json::from_slice(&target_ready.stdout).unwrap();
    assert_eq!(target_ready["valid"], true);
    assert_eq!(target_ready["readiness_checked"], false);

    let ready = directory.command(&["check", "--config-dir", "mock", "--format", "json"]);
    assert!(ready.status.success());
    let ready: Value = serde_json::from_slice(&ready.stdout).unwrap();
    assert_eq!(ready["valid"], true);
    assert_eq!(ready["readiness_checked"], true);

    let hanging = &layout.resolvers[1];
    let hanging_source = original.replacen("--!strict\n", "--!strict\nwhile true do end\n", 1);
    fs::write(root.join(hanging.path.as_str()), hanging_source).unwrap();
    let resource = directory.command(&[
        "check",
        "--config-dir",
        "mock",
        "--target",
        selected,
        "--format",
        "json",
    ]);
    assert_eq!(resource.status.code(), Some(2));
    assert!(resource.stderr.is_empty());
    let resource: Value = serde_json::from_slice(&resource.stdout).unwrap();
    assert_eq!(resource["valid"], false);
    assert_eq!(resource["readiness_checked"], false);
    assert_eq!(resource["diagnostics"][0]["phase"], "timeout");
    fs::write(root.join(hanging.path.as_str()), &original).unwrap();

    for (phase, initialization) in [
        (
            "memory",
            "local s = \"x\"\nfor _ = 1, 30 do s = s .. s end\n",
        ),
        (
            "limit",
            "local function dive(n: number): ()\n    if n == 0 then return end\n    pcall(dive, n - 1)\nend\ndive(17)\n",
        ),
    ] {
        let source = original.replacen("--!strict\n", &format!("--!strict\n{initialization}"), 1);
        fs::write(root.join(hanging.path.as_str()), source).unwrap();
        let resource = directory.command(&[
            "check",
            "--config-dir",
            "mock",
            "--target",
            selected,
            "--format",
            "json",
        ]);
        assert_eq!(resource.status.code(), Some(1));
        assert!(resource.stderr.is_empty());
        let resource: Value = serde_json::from_slice(&resource.stdout).unwrap();
        assert_eq!(resource["valid"], false);
        assert_eq!(resource["readiness_checked"], false);
        assert_eq!(resource["diagnostics"][0]["phase"], phase);
        fs::write(root.join(hanging.path.as_str()), &original).unwrap();
    }

    let mut child = Command::new("sh")
        .args([
            "-c",
            "sleep 0.1; exec \"$1\" mock check --config-dir mock",
            "broken-pipe",
            BINARY,
        ])
        .current_dir(&directory.0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let broken_pipe = child.wait_with_output().unwrap();
    assert!(broken_pipe.status.success());
    assert!(broken_pipe.stderr.is_empty());
}

#[test]
fn ai_ready_json_reports_required_reference_cycles_as_warnings_on_valid_scaffolds() {
    let _test_case = crate::support::test_case();
    let directory = Directory::new();
    fs::write(
        directory.0.join("schema.graphql"),
        "type A{id:ID! b:B!} type B{id:ID! a:A!} type Query{a:A}",
    )
    .unwrap();
    let init = directory.command(&["init", "--schema", "schema.graphql", "--config-dir", "mock"]);
    assert!(init.status.success());
    assert!(String::from_utf8_lossy(&init.stderr).contains("A.b -> B.a -> A"));

    let check = directory.command(&["check", "--config-dir", "mock", "--format", "json"]);
    assert!(check.status.success());
    assert!(check.stderr.is_empty());
    let report: Value = serde_json::from_slice(&check.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["severity"], "warning");
    assert_eq!(report["diagnostics"][0]["file"], Value::Null);
    assert_eq!(report["diagnostics"][0]["coordinate"], Value::Null);
    assert_eq!(report["diagnostics"][0]["member"], "A.b -> B.a -> A");
    assert_eq!(report["diagnostics"][0]["line"], Value::Null);
    assert_eq!(report["diagnostics"][0]["column"], Value::Null);
    assert_eq!(report["diagnostics"][0]["cause"], "MOCK_SCHEMA_CYCLE");
    assert_eq!(report["valid"], true);
    assert_eq!(report["readiness_checked"], true);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
}

#[test]
fn ai_ready_target_check_propagates_unrelated_hook_vm_limits() {
    let _test_case = crate::support::test_case();
    let directory = project(include_str!("../../fixtures/blog.graphql"), TEMPLATE);
    let root = directory.0.join("mock");
    let schema = read_project_schema(&root).unwrap();
    let layout = Layout::new(schema.ir()).unwrap();
    let selected = &layout.resolvers[0].path;
    let config_path = root.join("mock.luau");
    let disabled = fs::read_to_string(&config_path).unwrap();
    let enabled = disabled.replace("request = { before = false", "request = { before = true");
    assert_ne!(enabled, disabled);
    let slot = HookSlot::RequestBefore;
    let original = render_hook_module(slot, "return nil").unwrap();

    for (phase, initialization) in [
        (
            "memory",
            "local s = \"x\"\nfor _ = 1, 30 do s = s .. s end\n",
        ),
        (
            "limit",
            "local function dive(n: number): ()\n    if n == 0 then return end\n    pcall(dive, n - 1)\nend\ndive(17)\n",
        ),
    ] {
        fs::write(&config_path, &disabled).unwrap();
        let source = original.replacen("--!strict\n", &format!("--!strict\n{initialization}"), 1);
        fs::write(root.join(slot.path()), source).unwrap();
        update_project(&root).unwrap();
        fs::write(&config_path, &enabled).unwrap();
        update_project(&root).unwrap();

        let resource = directory.command(&[
            "check",
            "--config-dir",
            "mock",
            "--target",
            selected,
            "--format",
            "json",
        ]);
        assert_eq!(resource.status.code(), Some(1));
        assert!(resource.stderr.is_empty());
        let resource: Value = serde_json::from_slice(&resource.stdout).unwrap();
        assert_eq!(resource["valid"], false);
        assert_eq!(resource["readiness_checked"], false);
        assert_eq!(resource["diagnostics"][0]["phase"], phase);
        assert_eq!(resource["diagnostics"][0]["file"], slot.path());
    }
}
