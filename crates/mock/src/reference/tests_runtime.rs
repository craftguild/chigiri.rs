//! Runtime discovery and private-storage visibility are verified from executable inputs.

use std::fs;

use serde_json::Value;

use super::{Format, ReferenceDocument, Selection, Topic};
use crate::hooks::{HookSlot, StateContract};
use crate::luau::{Contract, HookCandidate};
use crate::value::ValueBudget;

#[test]
fn tl_01_09_14_16_runtime_names_are_exact_self_contained_and_executable() {
    let document = ReferenceDocument::build(None).unwrap();
    for name in ["stdlib", "request", "timeleap", "errors", "storage"] {
        let selection = Selection::new(Some(Topic::Runtime), Some(name.into())).unwrap();
        assert!(document.accepts(&selection));
        let value: Value =
            serde_json::from_slice(&document.render_selection(Format::Json, &selection).unwrap())
                .unwrap();
        let item = &value["content"]["items"][0];
        assert_eq!(item["name"], name);
        assert!(!item["signatures"].as_array().unwrap().is_empty());
        assert!(!item["examples"].as_array().unwrap().is_empty());
        if name == "stdlib" {
            assert!(
                item["limits"]
                    .to_string()
                    .contains("shared Luau modules are unsupported")
            );
            assert!(
                item["signatures"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|signature| {
                        signature.as_str() == Some("assert<T>(value: T, message: string?) -> T")
                    })
            );
            assert!(
                item["signatures"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|signature| {
                        signature
                            .as_str()
                            .is_some_and(|value| value.contains("position: number"))
                    })
            );
            for signature in ["os.time(", "os.date:", "os.difftime(", "os.clock("] {
                assert!(item["signatures"].as_array().unwrap().iter().any(|value| {
                    value
                        .as_str()
                        .is_some_and(|value| value.contains(signature))
                }));
            }
            assert!(!item["signatures"].to_string().contains("timeleap."));
        } else if name == "timeleap" {
            assert_eq!(
                item["signatures"],
                serde_json::json!([
                    "timeleap.travel_to(timestamp: number, options: TimeleapOptions?) -> ()",
                    "timeleap.reset(options: TimeleapOptions?) -> ()"
                ])
            );
            let text = item.to_string();
            assert!(text.contains("request-local") && text.contains("world = true"));
            assert!(
                text.contains("0 through 9,007,199,254,740,991 inclusive")
                    && text.contains("native UTC round-trip")
            );
        }
    }
    assert!(
        !document.accepts(&Selection::new(Some(Topic::Runtime), Some("clock".into())).unwrap())
    );
    assert!(
        !document.accepts(&Selection::new(Some(Topic::Runtime), Some("missing".into())).unwrap())
    );
}

#[test]
fn sa_07_storage_reference_example_checks_and_visibility_stays_scoped() {
    let _test_case = crate::vm_gate::test_case();
    let selection = Selection::new(Some(Topic::Runtime), Some("storage".into())).unwrap();
    let value: Value = serde_json::from_slice(
        &ReferenceDocument::build(None)
            .unwrap()
            .render_selection(Format::Json, &selection)
            .unwrap(),
    )
    .unwrap();
    let examples = value["content"]["items"][0]["examples"].as_array().unwrap();
    let example = |title: &str| {
        examples.iter().find(|item| item["title"] == title).unwrap()["code"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let public = example("Required public schema");
    let storage = example("storage.graphql");
    let user_seed = example("seeds/User.jsonl");
    let wallet_seed = example("seeds/Wallet.jsonl");
    let preference_seed = example("seeds/MockPreference.jsonl");
    let resolver = example("Query.user resolver body");
    let image_resolver = example("Image.viewerLabel resolver body");

    let fixture = crate::project::test_support::Fixture::new(&public);
    let project = fixture.base.join("storage-reference-example");
    crate::project::init_project(&[fixture.base.join("schema.graphql")], &project).unwrap();
    fs::write(project.join("storage.graphql"), storage).unwrap();
    crate::project::update_project(&project).unwrap();
    fs::write(project.join("seeds/User.jsonl"), user_seed).unwrap();
    fs::write(project.join("seeds/Wallet.jsonl"), wallet_seed).unwrap();
    fs::write(project.join("seeds/MockPreference.jsonl"), preference_seed).unwrap();
    let ir = crate::project::read_project_schema(&project)
        .unwrap()
        .ir()
        .clone();
    let source = |coordinate: &str, body: &str| {
        let (parent, name) = coordinate.split_once('.').unwrap();
        let parent = ir.graph().type_id(parent).unwrap();
        let field = ir.graph().types()[parent.0].fields[name];
        let id = ir
            .scripts()
            .iter()
            .position(|plan| plan.field == field)
            .map(crate::ir::ScriptId)
            .unwrap();
        crate::luau::render_module(&ir, id, body).unwrap()
    };
    fs::write(
        project.join("resolvers/query/user.luau"),
        source("Query.user", &resolver),
    )
    .unwrap();
    fs::write(
        project.join("resolvers/objects/Image/viewerLabel.luau"),
        source("Image.viewerLabel", &image_resolver),
    )
    .unwrap();
    crate::project::ActiveSnapshot::capture(&project)
        .unwrap()
        .check()
        .unwrap();

    let document = ReferenceDocument::build(Some(&ir)).unwrap();
    let types = Selection::new(Some(Topic::Types), None).unwrap();
    let types: Value =
        serde_json::from_slice(&document.render_selection(Format::Json, &types).unwrap()).unwrap();
    assert!(!types.to_string().contains("MockPreference"));
    assert!(!types.to_string().contains("verifiedAt"));
    assert!(!types.to_string().contains("ownerId"));
    assert!(types.to_string().contains("viewerLabel"));
    let models = Selection::new(Some(Topic::Models), Some("MockPreference".into())).unwrap();
    let models: Value =
        serde_json::from_slice(&document.render_selection(Format::Json, &models).unwrap()).unwrap();
    assert_eq!(models["content"]["items"][0]["visibility"], "storage");
    assert_eq!(
        models["content"]["items"][0]["source"],
        "storage/001.graphql"
    );
}

#[test]
fn authoring_guidance_storage_and_module_boundaries_reach_text_and_json() {
    let document = ReferenceDocument::build(None).unwrap();
    for (name, terms) in [
        (
            "storage",
            vec![
                "genuine model records unchanged",
                "snapshots",
                "public fields only",
                "MOCK_OUTPUT_PRIVATE_FIELD",
                "static check does not prove output provenance",
            ],
        ),
        (
            "stdlib",
            vec![
                "require and shared Luau modules are unsupported",
                "return error(...)",
                "canonical UTC format",
            ],
        ),
        ("request", vec!["table guard", "demo tokens"]),
    ] {
        let selection = Selection::new(Some(Topic::Runtime), Some(name.into())).unwrap();
        for format in [Format::Text, Format::Json] {
            let output =
                String::from_utf8(document.render_selection(format, &selection).unwrap()).unwrap();
            for term in &terms {
                assert!(output.contains(term), "{name}: {term}");
            }
        }
    }
}

#[test]
fn ab_12_optional_authentication_preparation_example_uses_declared_state() {
    let selection = Selection::new(Some(Topic::Runtime), Some("request".into())).unwrap();
    let value: Value = serde_json::from_slice(
        &ReferenceDocument::build(None)
            .unwrap()
            .render_selection(Format::Json, &selection)
            .unwrap(),
    )
    .unwrap();
    let examples = value["content"]["items"][0]["examples"].as_array().unwrap();
    let body = examples
        .iter()
        .find(|example| {
            example["title"]
                .as_str()
                .unwrap()
                .contains("authentication")
        })
        .unwrap()["code"]
        .as_str()
        .unwrap();
    assert!(body.contains("ctx.request.headers"));
    let ir = std::sync::Arc::new(
        crate::ir::MockIr::parse(vec![chigiri::core::Source::new(
            "schema.graphql",
            "type Query { ok: Boolean! }",
        )])
        .unwrap(),
    );
    let state = StateContract::from_json(
        r#"{"fields":{"authenticated":{"kind":"boolean"}},"initial":{"authenticated":false}}"#,
        &mut ValueBudget::default(),
    )
    .unwrap();
    let contract = Contract::with_state(ir, state).unwrap();
    let candidate = HookCandidate {
        slot: HookSlot::RequestBefore,
        source: crate::luau::render_hook_module(HookSlot::RequestBefore, body).unwrap(),
    };
    assert_eq!(contract.hooks(vec![candidate], None).unwrap().len(), 1);
}
