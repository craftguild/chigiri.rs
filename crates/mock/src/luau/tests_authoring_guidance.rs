//! Check published guard examples and retain minimal probes for dependency-release review.

use super::tests::{candidate, contract};
use crate::reference::{Format, ReferenceDocument, Selection, Topic};

fn example(topic: &str, title: &str) -> String {
    let document = ReferenceDocument::build(None).unwrap();
    let selection = Selection::new(Some(Topic::Runtime), Some(topic.into())).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&document.render_selection(Format::Json, &selection).unwrap())
            .unwrap();
    let body = json["content"]["items"][0]["examples"]
        .as_array()
        .unwrap()
        .iter()
        .find(|example| example["title"].as_str().unwrap().starts_with(title))
        .unwrap()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    let text =
        String::from_utf8(document.render_selection(Format::Text, &selection).unwrap()).unwrap();
    assert!(text.contains(title));
    assert!(
        !body.contains("::"),
        "examples must not require type assertions"
    );
    body
}

#[test]
fn authoring_guidance_reference_guards_check_without_casts() {
    let _case = crate::vm_gate::test_case();
    for (topic, title, schema) in [
        (
            "request",
            "Read duplicate-preserving headers",
            "type Query { value: [String!] }",
        ),
        (
            "request",
            "Require the demo Bearer token",
            "type Query { value: Boolean! }",
        ),
        (
            "stdlib",
            "Compare canonical UTC timestamps",
            "type Query { value(deadline: String!): Boolean! }",
        ),
    ] {
        let contract = contract(schema);
        let body = example(topic, title);
        contract
            .script(candidate(&contract, "Query.value", &body))
            .unwrap();
    }
}

#[test]
fn authoring_guidance_workflow_guard_bodies_check_verbatim() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { value(deadline: String!): Boolean! }");
    let workflow = include_str!("../../../../docs/mock-project-workflow.md");
    let mut checked = 0;
    for section in workflow.split("```luau\n").skip(1) {
        let body = section.split("```").next().unwrap();
        if body.contains("local request = ctx.request") || body.contains("local nowText = os.date")
        {
            assert!(!body.contains("::"));
            contract
                .script(candidate(&contract, "Query.value", body))
                .unwrap();
            checked += 1;
        }
    }
    assert_eq!(checked, 2);
}

#[test]
fn authoring_guidance_guards_do_not_make_unguarded_access_valid() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: String! }");
    for body in [
        "return ctx.request.method",
        "local value = os.date('!%Y-%m-%dT%H:%M:%SZ', assert(os.time())) return value",
    ] {
        assert_eq!(
            contract
                .script(candidate(&contract, "Query.value", body))
                .err()
                .unwrap()
                .phase,
            "type"
        );
    }
}

#[test]
fn authoring_guidance_published_checker_refinement_probes() {
    let _case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    // No Chigiri transformation or os declaration is involved. On a future
    // unmodified registry release, revisit these expectations and the guidance.
    let date_guard = r#"--!strict
local function later(value: (string | {year: number})?): boolean
    if type(value) ~= "string" then error("not a string") end
    return "2026-09-27T13:03:19Z" > value
end
return later"#;
    luaur::compile(date_guard).unwrap();
    assert!(luaur::check(date_guard).is_err());
    luaur::check(&date_guard.replace("then error(", "then return error(")).unwrap();
    luaur::check("--!strict\nlocal a: string = 'b'\nlocal b: string = 'a'\nreturn a > b").unwrap();

    let definitions = "declare extern type Null with\nend\n";
    let request_guard = r#"--!strict
local function method(request: {method: string} | Null, null: Null): string
    if request == null then return "missing" end
    return request.method
end
return method"#;
    luaur::compile(request_guard).unwrap();
    assert!(luaur::check_with_definitions(request_guard, definitions).is_err());
    let guarded = request_guard.replace("request == null", "type(request) ~= 'table'");
    luaur::check_with_definitions(&guarded, definitions).unwrap();
}
