//! Verifies short landing pages, JSON topics, schema models, and navigation
//! derived from the same document.

use super::{Format, ReferenceDocument, Selection, Topic, topic_types::TopicDocument};
use crate::ir::MockIr;
use chigiri::core::Source;
use serde_json::{Value, json};

fn selection(topic: Option<Topic>, name: Option<&str>) -> Selection {
    Selection::new(topic, name.map(str::to_owned)).unwrap()
}

fn assert_sections(text: &str, sections: &[&str]) {
    let mut previous = 0;
    for section in sections {
        let position = text[previous..]
            .find(&format!("\n{section}\n"))
            .map(|position| previous + position)
            .unwrap_or_else(|| panic!("missing man section {section:?}"));
        previous = position + section.len() + 2;
    }
}

#[test]
fn ref_02_default_text_is_short_and_all_keeps_complete_reference() {
    let document = ReferenceDocument::build(None).unwrap();
    let landing = document
        .render_selection(Format::Text, &selection(None, None))
        .unwrap();
    let landing = String::from_utf8(landing).unwrap();
    assert!(landing.lines().count() <= 30);
    assert!(landing.starts_with("CHIGIRI-MOCK-REFERENCE(1)\n"));
    assert_sections(
        &landing,
        &[
            "NAME",
            "SYNOPSIS",
            "DESCRIPTION",
            "TOPICS",
            "EXAMPLES",
            "SEE ALSO",
        ],
    );
    for topic in [
        "directives",
        "methods",
        "hooks",
        "models",
        "types",
        "resolvers",
        "limits",
        "runtime",
        "all",
    ] {
        assert!(landing.contains(topic));
    }
    assert!(!landing.contains("\nAPI TYPES\n") && !landing.contains("\nSEMANTICS AND ERRORS\n"));
    let all = document
        .render_selection(Format::Text, &selection(Some(Topic::All), None))
        .unwrap();
    assert_eq!(all, document.render(Format::Text).unwrap());
    let all = String::from_utf8(all).unwrap();
    assert!(all.starts_with("CHIGIRI-MOCK-REFERENCE(1)\n"));
    assert_sections(
        &all,
        &[
            "NAME",
            "SYNOPSIS",
            "DESCRIPTION",
            "DIRECTIVES",
            "METHODS",
            "API TYPES",
            "SEMANTICS AND ERRORS",
            "LIMITS",
            "EXAMPLES",
            "SEE ALSO",
        ],
    );
}

#[test]
fn ref_02_topic_indexes_and_details_use_stable_man_sections() {
    let document = ReferenceDocument::build(None).unwrap();
    let index = document
        .render_selection(Format::Text, &selection(Some(Topic::Methods), None))
        .unwrap();
    let index = String::from_utf8(index).unwrap();
    assert_sections(
        &index,
        &["NAME", "SYNOPSIS", "DESCRIPTION", "ITEMS", "SEE ALSO"],
    );
    let detail = document
        .render_selection(
            Format::Text,
            &selection(Some(Topic::Directives), Some("representation")),
        )
        .unwrap();
    let detail = String::from_utf8(detail).unwrap();
    assert_sections(
        &detail,
        &[
            "NAME",
            "SYNOPSIS",
            "DESCRIPTION",
            "SYNTAX",
            "ARGUMENTS",
            "EXAMPLES",
            "RULES",
            "SEE ALSO",
        ],
    );
}

#[test]
fn ab_11_all_six_hooks_support_exact_text_and_json_selection() {
    let document = ReferenceDocument::build(None).unwrap();
    for name in [
        "request.before",
        "request.after",
        "operation.before",
        "operation.after",
        "field.before",
        "field.after",
    ] {
        let selected = selection(Some(Topic::Hooks), Some(name));
        assert!(document.accepts(&selected));
        let text =
            String::from_utf8(document.render_selection(Format::Text, &selected).unwrap()).unwrap();
        assert!(text.contains(&format!("chigiri mock reference hooks {name} [OPTIONS]")));
        let bytes = document.render_selection(Format::Json, &selected).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["selection"], json!({"topic":"hooks", "name":name}));
        assert_eq!(value["content"]["kind"], "hooks");
        assert_eq!(value["content"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(value["content"]["items"][0]["name"], name);
        assert_eq!(
            value["content"]["items"][0]["logging_format"],
            "chigiri-mock-log/v5"
        );
        assert_eq!(
            value["content"]["items"][0]["source_positions_nullable"],
            true
        );
        let mut obsolete = value.clone();
        obsolete["content"]["items"][0]["logging_format"] = json!("chigiri-mock-log/v2");
        assert!(serde_json::from_value::<TopicDocument>(obsolete).is_err());
    }
    assert!(!document.accepts(&selection(Some(Topic::Hooks), Some("request.other"))));
}

#[test]
fn ref_07_10_topic_json_is_closed_deterministic_and_single_name_is_self_contained() {
    let document = ReferenceDocument::build(None).unwrap();
    let selection = selection(Some(Topic::Methods), Some("find"));
    let bytes = document.render_selection(Format::Json, &selection).unwrap();
    assert_eq!(
        bytes,
        document.render_selection(Format::Json, &selection).unwrap()
    );
    let topic: TopicDocument = serde_json::from_slice(&bytes).unwrap();
    let value: Value = serde_json::to_value(topic).unwrap();
    assert_eq!(value["format"], "chigiri-mock-reference-topic/v9");
    assert_eq!(value["reference_format"], "chigiri-mock-reference/v13");
    assert_eq!(
        value["selection"],
        json!({"topic":"methods", "name":"find"})
    );
    assert_eq!(value["content"]["kind"], "methods");
    let item = &value["content"]["items"][0];
    assert_eq!(item["name"], "find");
    assert!(item["signature"].as_str().unwrap().contains(":find("));
    assert!(item["rules"][0].as_str().unwrap().contains("snapshot"));
    assert!(!item["examples"].as_array().unwrap().is_empty());
    for pointer in ["", "/selection", "/content/items/0"] {
        let mut bad = value.clone();
        bad.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(true));
        assert!(serde_json::from_value::<TopicDocument>(bad).is_err());
    }
    let mut wrong = value.clone();
    wrong["selection"]["topic"] = json!("directives");
    assert!(serde_json::from_value::<TopicDocument>(wrong).is_err());
    let mut wrong_name = value.clone();
    wrong_name["selection"]["name"] = json!("count");
    assert!(serde_json::from_value::<TopicDocument>(wrong_name).is_err());
    let mut checked = value;
    checked["readiness_checked"] = json!(true);
    assert!(serde_json::from_value::<TopicDocument>(checked).is_err());
    for (field, old) in [
        ("format", "chigiri-mock-reference-topic/v6"),
        ("reference_format", "chigiri-mock-reference/v10"),
        ("api_version", "chigiri-mock-api/v4"),
    ] {
        let mut old_document: Value = serde_json::from_slice(&bytes).unwrap();
        old_document[field] = json!(old);
        assert!(serde_json::from_value::<TopicDocument>(old_document).is_err());
    }
}

#[test]
fn ref_08_schema_models_are_exact_and_common_has_no_fictional_model() {
    let common = ReferenceDocument::build(None).unwrap();
    let models = selection(Some(Topic::Models), None);
    let text = String::from_utf8(common.render_selection(Format::Text, &models).unwrap()).unwrap();
    assert!(text.contains("No models are available."));
    assert!(!text.contains("User") && !text.contains("Post"));
    assert!(!common.accepts(&selection(Some(Topic::Models), Some("User"))));

    let ir = MockIr::parse(vec![Source::new(
        "domain.graphql",
        "type Account { id: ID! name: String! } type Query { account: Account }",
    )])
    .unwrap();
    let document = ReferenceDocument::build(Some(&ir)).unwrap();
    assert!(document.accepts(&selection(Some(Topic::Models), Some("Account"))));
    assert!(!document.accepts(&selection(Some(Topic::Models), Some("User"))));
    let value: Value = serde_json::from_slice(
        &document
            .render_selection(
                Format::Json,
                &selection(Some(Topic::Models), Some("Account")),
            )
            .unwrap(),
    )
    .unwrap();
    let item = &value["content"]["items"][0];
    assert_eq!(item["name"], "Account");
    assert_eq!(item["key"], "id");
    assert_eq!(item["slots"][1]["name"], "name");
    assert!(
        item["methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "find")
    );
}

#[test]
fn sa_06_types_topic_excludes_private_abstract_implementors() {
    let ir = MockIr::parse_with_storage(
        vec![Source::new(
            "schema.graphql",
            "interface Node{id:ID!} type Public implements Node{id:ID!} type Query{node:Node}",
        )],
        vec![Source::new(
            "storage.graphql",
            "type Hidden implements Node{id:ID! secret:String!}",
        )],
    )
    .unwrap();
    let document = ReferenceDocument::build(Some(&ir)).unwrap();
    let value: Value = serde_json::from_slice(
        &document
            .render_selection(Format::Json, &selection(Some(Topic::Types), Some("Node")))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        value["content"]["items"][0]["possible_types"],
        json!(["Public"])
    );
}

#[test]
fn ref_09_11_full_json_is_identical_for_default_and_all_and_topic_is_separate() {
    let document = ReferenceDocument::build(None).unwrap();
    let default = document
        .render_selection(Format::Json, &selection(None, None))
        .unwrap();
    let all = document
        .render_selection(Format::Json, &selection(Some(Topic::All), None))
        .unwrap();
    assert_eq!(default, all);
    assert_eq!(default, document.render(Format::Json).unwrap());
    let value: Value = serde_json::from_slice(&default).unwrap();
    assert_eq!(value["format"], "chigiri-mock-reference/v13");
    assert_eq!(value["directives"].as_array().unwrap().len(), 9);
    assert!(value.get("selection").is_none());
}

#[test]
fn ref_08_type_and_resolver_details_are_exact_and_self_contained() {
    let ir = MockIr::parse(vec![Source::new(
        "domain.graphql",
        "type Account { id: ID! name: String! } type Query { account(id: ID!): Account }",
    )])
    .unwrap();
    let document = ReferenceDocument::build(Some(&ir)).unwrap();

    let ty: Value = serde_json::from_slice(
        &document
            .render_selection(
                Format::Json,
                &selection(Some(Topic::Types), Some("Account")),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(ty["content"]["kind"], "types");
    assert_eq!(ty["content"]["items"][0]["name"], "Account");
    assert_eq!(
        ty["content"]["items"][0]["fields"][1]["value_type"],
        "String!"
    );

    let resolver: Value = serde_json::from_slice(
        &document
            .render_selection(
                Format::Json,
                &selection(Some(Topic::Resolvers), Some("Query.account")),
            )
            .unwrap(),
    )
    .unwrap();
    let item = &resolver["content"]["items"][0];
    assert_eq!(resolver["content"]["kind"], "resolvers");
    assert_eq!(item["module_path"], "resolvers/query/account.luau");
    assert_eq!(item["arguments"][0]["value_type"], "ID!");
    assert_eq!(item["returns"], "Account");
    assert_eq!(item["scopes"][0]["scope"], "query_root");
    assert!(item["scopes"][0]["models"][0]["methods"].is_array());
    assert!(!document.accepts(&selection(Some(Topic::Resolvers), Some("Query.missing"),)));
}

#[test]
fn ref_07_hook_and_limit_topics_keep_shared_contract_values() {
    let document = ReferenceDocument::build(None).unwrap();
    let hooks: Value = serde_json::from_slice(
        &document
            .render_selection(Format::Json, &selection(Some(Topic::Hooks), None))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(hooks["content"]["items"].as_array().unwrap().len(), 6);
    assert!(
        hooks["content"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hook| { hook["enabled"].is_null() && hook["models_read_only"] == true })
    );

    let limits: Value = serde_json::from_slice(
        &document
            .render_selection(Format::Json, &selection(Some(Topic::Limits), None))
            .unwrap(),
    )
    .unwrap();
    let host_work = limits["content"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|limit| limit["id"] == "host.work")
        .unwrap();
    assert_eq!(host_work["value"], crate::api::WORK_LIMIT);
    assert_eq!(host_work["scope"], "scope");
}

#[test]
fn ref_08_two_models_and_renamed_root_produce_only_real_model_names() {
    let ir = MockIr::parse(vec![Source::new(
        "two-domains.graphql",
        "schema { query: Reading } type Alpha { id: ID! } type Beta { code: String! } type Reading { alpha: Alpha beta: Beta }",
    )])
    .unwrap();
    let document = ReferenceDocument::build(Some(&ir)).unwrap();
    let models: Value = serde_json::from_slice(
        &document
            .render_selection(Format::Json, &selection(Some(Topic::Models), None))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        models["content"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Alpha", "Beta"]
    );
    assert!(!document.accepts(&selection(Some(Topic::Models), Some("Reading"))));
}
