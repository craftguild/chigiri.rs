//! Exercises short listings, topic and name selection, partial JSON, and usage
//! errors through the real binary.

use super::harness::Directory;
use serde_json::{Value, json};
use std::fs;

#[test]
fn ref_01_02_cli_has_eight_topics_short_landing_and_exact_name_errors() {
    let directory = Directory::new();
    let landing = directory.run(&["reference"]);
    assert!(landing.status.success());
    assert!(landing.stderr.is_empty());
    let landing = String::from_utf8(landing.stdout).unwrap();
    assert!(landing.lines().count() <= 30);
    assert!(landing.starts_with("CHIGIRI-MOCK-REFERENCE(1)\n"));
    for section in [
        "NAME",
        "SYNOPSIS",
        "DESCRIPTION",
        "TOPICS",
        "EXAMPLES",
        "SEE ALSO",
    ] {
        assert!(landing.contains(&format!("\n{section}\n")));
    }
    for topic in [
        "directives",
        "methods",
        "hooks",
        "models",
        "types",
        "resolvers",
        "limits",
        "all",
    ] {
        assert!(landing.contains(topic));
    }
    let detail = directory.run(&["reference", "directives", "representation"]);
    assert!(detail.status.success());
    let detail = String::from_utf8(detail.stdout).unwrap();
    assert!(detail.contains("@representation"));
    assert!(detail.contains("ChigiriMockRepresentation!"));
    assert!(detail.contains("Type is required."));
    for section in [
        "NAME",
        "SYNOPSIS",
        "DESCRIPTION",
        "SYNTAX",
        "ARGUMENTS",
        "EXAMPLES",
        "SEE ALSO",
    ] {
        assert!(detail.contains(&format!("\n{section}\n")));
    }
    for args in [
        vec!["reference", "unknown"],
        vec!["reference", "hooks", "request.other"],
        vec!["reference", "limits", "host.work"],
        vec!["reference", "all", "extra"],
        vec!["reference", "methods", "missing"],
        vec!["reference", "directives", "missing"],
    ] {
        let output = directory.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        if args.len() == 3 && args[1] != "unknown" {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains(&format!("chigiri mock reference {}", args[1])));
        }
    }
}

#[test]
fn ab_11_cli_selects_every_hook_in_text_and_json() {
    let directory = Directory::new();
    for name in [
        "request.before",
        "request.after",
        "operation.before",
        "operation.after",
        "field.before",
        "field.after",
    ] {
        let text = directory.run(&["reference", "hooks", name]);
        assert!(
            text.status.success(),
            "{}",
            String::from_utf8_lossy(&text.stderr)
        );
        assert!(text.stderr.is_empty());
        let text = String::from_utf8(text.stdout).unwrap();
        assert!(text.contains(&format!("chigiri mock reference hooks {name} [OPTIONS]")));

        let json = directory.run(&["reference", "hooks", name, "--format", "json"]);
        assert!(json.status.success());
        assert!(json.stderr.is_empty());
        let value: Value = serde_json::from_slice(&json.stdout).unwrap();
        assert_eq!(value["selection"], json!({"topic":"hooks", "name":name}));
        assert_eq!(value["content"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(value["content"]["items"][0]["name"], name);
    }
}

#[test]
fn ref_08_10_schema_topic_json_selects_real_model_and_user_directive() {
    let directory = Directory::new();
    fs::write(
        directory.0.join("domain.graphql"),
        "\"Audit\" directive @audit(level: Int! = 1) on FIELD_DEFINITION\ntype Account { id: ID! name: String! } type Query { account: Account @audit }",
    )
    .unwrap();
    let model = directory.run(&[
        "reference",
        "models",
        "Account",
        "--schema",
        "domain.graphql",
        "--format",
        "json",
    ]);
    assert!(model.status.success());
    assert!(model.stderr.is_empty());
    let model: Value = serde_json::from_slice(&model.stdout).unwrap();
    assert_eq!(model["format"], "chigiri-mock-reference-topic/v9");
    assert_eq!(
        model["selection"],
        json!({"topic":"models", "name":"Account"})
    );
    assert_eq!(model["content"]["kind"], "models");
    assert_eq!(model["content"]["items"][0]["name"], "Account");
    assert_eq!(model["content"]["items"][0]["key"], "id");

    let ty = directory.run(&[
        "reference",
        "types",
        "Account",
        "--schema",
        "domain.graphql",
        "--format",
        "json",
    ]);
    assert!(ty.status.success());
    let ty: Value = serde_json::from_slice(&ty.stdout).unwrap();
    assert_eq!(ty["content"]["kind"], "types");
    assert_eq!(ty["content"]["items"][0]["fields"][1]["name"], "name");

    let resolver = directory.run(&[
        "reference",
        "resolvers",
        "Query.account",
        "--schema",
        "domain.graphql",
        "--format",
        "json",
    ]);
    assert!(resolver.status.success());
    let resolver: Value = serde_json::from_slice(&resolver.stdout).unwrap();
    assert_eq!(resolver["content"]["kind"], "resolvers");
    assert_eq!(
        resolver["content"]["items"][0]["module_path"],
        "resolvers/query/account.luau"
    );

    let directives = directory.run(&[
        "reference",
        "directives",
        "--schema",
        "domain.graphql",
        "--format",
        "json",
    ]);
    assert!(directives.status.success());
    let directives: Value = serde_json::from_slice(&directives.stdout).unwrap();
    let audit = directives["content"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "audit")
        .unwrap();
    assert_eq!(audit["class"], "user_declared");
    assert_eq!(audit["execution"], "declaration_only");
    assert_eq!(audit["declared_description"]["origin"], "user_sdl");
    let missing = directory.run(&["reference", "models", "User", "--schema", "domain.graphql"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
}

#[test]
fn ref_14_type_description_escapes_terminal_controls_in_text() {
    let directory = Directory::new();
    fs::write(
        directory.0.join("controls.graphql"),
        "\"\"\"visible\u{1b}[31m\u{202e}spoof\"\"\" type Query { value: Int }",
    )
    .unwrap();
    let output = directory.run(&[
        "reference",
        "types",
        "Query",
        "--schema",
        "controls.graphql",
    ]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains('\u{202e}'));
    assert!(text.contains("\\u{1b}[31m\\u{202e}spoof"));
}

#[test]
fn ref_09_11_default_and_all_json_are_the_same_complete_v7_document() {
    let directory = Directory::new();
    let default = directory.run(&["reference", "--format", "json"]);
    let all = directory.run(&["reference", "all", "--format", "json"]);
    assert!(default.status.success() && all.status.success());
    assert_eq!(default.stdout, all.stdout);
    let document: Value = serde_json::from_slice(&default.stdout).unwrap();
    assert_eq!(document["format"], "chigiri-mock-reference/v13");
    assert_eq!(document["readiness_checked"], false);
    assert_eq!(document["directives"].as_array().unwrap().len(), 9);
    assert!(document.get("selection").is_none());
}

#[test]
fn type_topic_expands_relation_keys_without_dangling_internal_ids() {
    let directory = Directory::new();
    fs::write(
        directory.0.join("relations.graphql"),
        "type User { id: ID! posts: [Post!]! @relation } \
         type Post { id: ID! authorId: ID! author: User! @relation(fields: [\"authorId\"], references: [\"id\"]) } \
         type Query { posts: [Post!]! }",
    )
    .unwrap();
    let output = directory.run(&[
        "reference",
        "types",
        "Post",
        "--schema",
        "relations.graphql",
        "--format",
        "json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    let fields = document["content"]["items"][0]["fields"]
        .as_array()
        .unwrap();
    let author = fields
        .iter()
        .find(|field| field["name"] == "author")
        .unwrap();
    let relation = &author["role"]["relation"];
    assert_eq!(author["role"]["kind"], "owner");
    assert_eq!(relation["owner_model"], "Post");
    assert_eq!(relation["target_model"], "User");
    assert_eq!(relation["storage_keys"], json!(["authorId"]));
    assert_eq!(relation["reference_keys"], json!(["id"]));
    let serialized = serde_json::to_string(&document).unwrap();
    assert!(!serialized.contains("relation/") && !serialized.contains("slot/"));
}

#[test]
fn type_and_resolver_text_keep_representation_constraints_and_defaults() {
    let directory = Directory::new();
    fs::write(
        directory.0.join("contracts.graphql"),
        "scalar Price @representation(type: INTEGER, min: 1, max: 100) \
         input PriceFilter { limit: Int! = 20 } \
         type Item { id: ID! price: Price! } \
         type Query { items(limit: Int! = 20, filter: PriceFilter): [Item!]! }",
    )
    .unwrap();

    let render = |topic: &str, name: &str| {
        let output = directory.run(&["reference", topic, name, "--schema", "contracts.graphql"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let price = render("types", "Price");
    assert!(price.contains("type=INTEGER"));
    assert!(price.contains("min=1"));
    assert!(price.contains("max=100"));

    let query = render("types", "Query");
    let input = render("types", "PriceFilter");
    let resolver = render("resolvers", "Query.items");
    for text in [&query, &input, &resolver] {
        assert!(text.contains("limit: Int! = 20"), "{text}");
    }
}
