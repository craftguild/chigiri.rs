//! Compares the six hook slots and configured state types against independent
//! JSON expectations and generated modules.

use super::{Format, InputKind, ReferenceDocument};
use crate::hooks::{HookSettings, HookSlot, StateContract};
use crate::luau::{Contract, HookCandidate, render_hook_module};
use crate::value::ValueBudget;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::Arc;

fn shape<'a>(doc: &'a Value, id: &str) -> &'a Value {
    &doc["api_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap()["shape"]
}
fn document(doc: &ReferenceDocument) -> Value {
    serde_json::from_slice(&doc.render(Format::Json).unwrap()).unwrap()
}

#[test]
fn hk_03_19_reference_v3_has_exact_six_capabilities_complete_types_and_closed_old_versions() {
    let doc = ReferenceDocument::build(None).unwrap();
    let value = document(&doc);
    assert!(super::tests_type_graph::graph_is_closed(&value));
    assert_eq!(value["format"], "chigiri-mock-reference/v13");
    assert_eq!(value["api_version"], "chigiri-mock-api/v10");
    assert_eq!(value["state_configured"], false);
    assert_eq!(value["rejection"]["bodyless_statuses"], json!([]));
    assert_eq!(value["rejection"]["invalid_status"], 500);
    let accepted = value["rejection"]["accepted_statuses"].as_array().unwrap();
    assert!(
        accepted.contains(&json!(200))
            && accepted.contains(&json!(401))
            && accepted.contains(&json!(511))
    );
    assert!(!accepted.contains(&json!(499)) && !accepted.contains(&json!(600)));
    assert_eq!(value["logging"]["format"], "chigiri-mock-log/v5");
    assert_eq!(value["logging"]["source_positions_nullable"], true);
    assert!(
        value["logging"]["diagnostic_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("member"))
    );
    assert_eq!(
        shape(&value, "ChigiriState"),
        &json!({"kind":"map","value_type_id":"json","string_keys":true})
    );
    assert_eq!(value["contexts"].as_array().unwrap().len(), 9);
    let hooks = value["hooks"].as_array().unwrap();
    assert_eq!(hooks.len(), 6);
    for (index, (slot, alias, phase, timing, patch)) in [
        (
            "request.before",
            "ChigiriRequestBefore",
            "request",
            "before",
            true,
        ),
        (
            "request.after",
            "ChigiriRequestAfter",
            "request",
            "after",
            false,
        ),
        (
            "operation.before",
            "ChigiriOperationBefore",
            "operation",
            "before",
            true,
        ),
        (
            "operation.after",
            "ChigiriOperationAfter",
            "operation",
            "after",
            false,
        ),
        (
            "field.before",
            "ChigiriFieldBefore",
            "field",
            "before",
            false,
        ),
        ("field.after", "ChigiriFieldAfter", "field", "after", false),
    ]
    .into_iter()
    .enumerate()
    {
        let hook = &hooks[index];
        assert_eq!(hook["slot"], slot);
        assert_eq!(hook["phase"], phase);
        assert_eq!(hook["timing"], timing);
        assert_eq!(hook["enabled"], Value::Null);
        assert_eq!(hook["state_patch"], patch);
        assert_eq!(hook["reject"], timing == "before");
        assert_eq!(hook["models_read_only"], true);
        assert_eq!(hook["module_path"], format!("hooks/{phase}/{timing}.luau"));
        assert_eq!(hook["context_type_id"], format!("{alias}Context"));
        let context = shape(&value, &format!("{alias}Context"));
        assert_eq!(context["closed"], true);
        let names: BTreeSet<_> = context["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field["name"].as_str().unwrap())
            .collect();
        let mut expected = BTreeSet::from([
            "scope",
            "null",
            "models",
            "state",
            "log",
            "jsonObject",
            "jsonArray",
        ]);
        if timing == "before" {
            expected.extend(["reject", "request"]);
        } else {
            expected.insert("result");
        }
        if phase != "request" {
            expected.insert("operation");
        }
        if phase == "field" {
            expected.insert("field");
        }
        assert_eq!(names, expected);
        let returned = shape(&value, &format!("{alias}Result"));
        assert_eq!(
            returned["variant_type_ids"].as_array().unwrap().len(),
            if patch { 2 } else { 1 }
        );
        assert_eq!(returned["variant_type_ids"][0], "hook_nil");
        let capability = value["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == format!("hook/{slot}"))
            .unwrap();
        assert_eq!(capability["scope_value"], "read");
        assert_eq!(
            capability["method_ids"],
            json!(["model.count", "model.find"])
        );
    }
    let log = shape(&value, "ChigiriHookLog");
    assert_eq!(log["minimum_arity"], 2);
    assert_eq!(log["maximum_arity"], 3);
    assert_eq!(log["arguments"][2]["required"], false);
    assert_eq!(
        shape(&value, "ChigiriHookLog.level")["values"],
        json!(["debug", "info", "warn", "error"])
    );
    assert_eq!(
        shape(&value, "ChigiriHookReject")["returns_type_id"],
        "hook_never"
    );
    assert_eq!(shape(&value, "hook_text")["max_bytes"], 1_048_576);
    assert_eq!(shape(&value, "json_string")["max_bytes"], 4096);
    let text = String::from_utf8(doc.render(Format::Text).unwrap()).unwrap();
    for expected in [
        "request.before",
        "field.after",
        "state_configured: false",
        "\\xHH",
        "Authorization/Cookie",
        "log_loss",
        "timestamp",
    ] {
        assert!(text.contains(expected), "{expected}");
    }
    for (field, versions) in [
        (
            "format",
            ["chigiri-mock-reference/v1", "chigiri-mock-reference/v2"],
        ),
        (
            "api_version",
            ["chigiri-mock-api/v1", "chigiri-mock-api/v2"],
        ),
    ] {
        for version in versions {
            let mut bad = value.clone();
            bad[field] = json!(version);
            assert!(serde_json::from_value::<ReferenceDocument>(bad).is_err());
        }
    }
    let mut bad = value.clone();
    bad["logging"]["format"] = json!("chigiri-mock-log/v2");
    assert!(serde_json::from_value::<ReferenceDocument>(bad).is_err());
    let mut bad = value.clone();
    bad["hooks"][0]["next"] = json!(true);
    assert!(serde_json::from_value::<ReferenceDocument>(bad).is_err());
    let mut bad = value;
    bad["hooks"][0]["slot"] = json!("request.other");
    assert!(serde_json::from_value::<ReferenceDocument>(bad).is_err());
}

#[test]
fn hk_02_19_configured_reference_preserves_all_state_kinds_without_initial_secrets() {
    let ir = Arc::new(
        crate::ir::MockIr::parse(vec![chigiri::core::Source::new(
            "blog.graphql",
            include_str!("../../fixtures/blog.graphql"),
        )])
        .unwrap(),
    );
    let state=StateContract::from_json(r#"{"fields":{"flag":{"kind":"boolean"},"count":{"kind":"integer"},"ratio":{"kind":"number"},"text":{"kind":"string"},"list":{"kind":"list","of":{"kind":"string"}},"nested":{"kind":"object","fields":{"name":{"kind":"string"}}},"optional":{"kind":"nullable","of":{"kind":"string"}}},"initial":{"flag":false,"count":0,"ratio":0.5,"text":"INITIAL_SECRET","list":[],"nested":{"name":"VALUE_SECRET"}}}"#,&mut ValueBudget::default()).unwrap();
    let mut settings = HookSettings::default();
    settings.request.before = true;
    let doc = ReferenceDocument::from_configuration(
        InputKind::Project,
        &ir,
        &state,
        &settings,
        &crate::events::EventSettings::default(),
    )
    .unwrap();
    let value = document(&doc);
    assert!(super::tests_type_graph::graph_is_closed(&value));
    assert_eq!(value["state_configured"], true);
    assert_eq!(value["readiness_checked"], false);
    assert_eq!(value["hooks"][0]["enabled"], true);
    assert_eq!(value["hooks"][1]["enabled"], false);
    let state_fields = shape(&value, "ChigiriState")["fields"].as_array().unwrap();
    assert_eq!(state_fields.len(), 7);
    assert!(state_fields.iter().all(|field| field["required"] == true));
    assert!(
        shape(&value, "ChigiriStatePatch")["fields"]
            .as_array()
            .unwrap()
            .iter()
            .all(|field| field["required"] == false)
    );
    assert_eq!(
        shape(&value, "ChigiriState.count")["representation"],
        "safe_integer"
    );
    assert_eq!(
        shape(&value, "ChigiriState.list")["item_type_id"],
        "json_string"
    );
    assert_eq!(
        shape(&value, "ChigiriState.optional")["variant_type_ids"],
        json!(["json_string", "null_sentinel"])
    );
    assert_eq!(shape(&value, "ChigiriState.nested")["closed"], true);
    for format in [Format::Text, Format::Json] {
        let text = String::from_utf8(doc.render(format).unwrap()).unwrap();
        assert!(!text.contains("INITIAL_SECRET"));
        assert!(!text.contains("VALUE_SECRET"));
    }
    assert!(ReferenceDocument::from_input(InputKind::Project, Some(&ir)).is_err());
    assert!(
        ReferenceDocument::from_configuration(
            InputKind::Schema,
            &ir,
            &state,
            &settings,
            &crate::events::EventSettings::default()
        )
        .is_err()
    );
    let contract = Contract::with_state(ir, state).unwrap();
    let mut candidates = Vec::new();
    for slot in HookSlot::ALL {
        let example = value["examples"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == format!("hook-example/{}", slot.name()))
            .unwrap();
        candidates.push(HookCandidate {
            slot,
            source: render_hook_module(slot, example["luau"].as_str().unwrap()).unwrap(),
        });
    }
    assert_eq!(contract.hooks(candidates, None).unwrap().len(), 6);
}

#[test]
fn hk_19_repeated_state_alias_stops_before_unbounded_reference_expansion() {
    use crate::hooks::{StateFields, StateType};
    use chigiri::core::{Object, Value};

    let parent = "P".repeat(4000);
    let fields: StateFields = (0..3000)
        .map(|index| (format!("n{index}"), StateType::Integer))
        .collect();
    let initial: Object = fields
        .keys()
        .map(|name| (name.clone(), Value::Int(0)))
        .collect();
    let state = StateContract::checked(
        StateFields::from([(parent.clone(), StateType::Object { fields })]),
        &Value::Object([(parent, Value::Object(initial))].into_iter().collect()),
        &mut ValueBudget::default(),
    )
    .unwrap();
    let ir = crate::ir::MockIr::parse(vec![chigiri::core::Source::new(
        "blog.graphql",
        include_str!("../../fixtures/blog.graphql"),
    )])
    .unwrap();
    let result = ReferenceDocument::from_configuration(
        InputKind::Project,
        &ir,
        &state,
        &HookSettings::default(),
        &crate::events::EventSettings::default(),
    );
    let Err(error) = result else {
        panic!("reference type output must remain within the actual state capacity");
    };
    assert_eq!(error.to_string(), "reference exceeds 16 MiB");
}
