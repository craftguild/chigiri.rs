//! Verifies that the directive catalog matches the semantic schema IR and uses
//! a closed JSON representation.

use super::{Format, ReferenceDocument};
use crate::ir::MockIr;
use chigiri::core::Source;
use serde_json::{Value, json};

fn value(document: &ReferenceDocument) -> Value {
    serde_json::from_slice(&document.render(Format::Json).unwrap()).unwrap()
}

#[test]
fn ref_03_04_standard_and_mock_directives_share_semantic_signatures_and_choices() {
    let document = ReferenceDocument::build(None).unwrap();
    let wire = value(&document);
    assert_eq!(wire["format"], "chigiri-mock-reference/v13");
    let directives = wire["directives"].as_array().unwrap();
    assert_eq!(
        directives
            .iter()
            .map(|directive| directive["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "skip",
            "include",
            "deprecated",
            "specifiedBy",
            "oneOf",
            "relation",
            "virtual",
            "constraint",
            "representation"
        ]
    );
    for directive in &directives[..5] {
        assert_eq!(directive["class"], "graphql_standard");
        assert_eq!(directive["execution"], "graphql");
    }
    for directive in &directives[5..] {
        assert_eq!(directive["class"], "chigiri_mock");
        assert_eq!(directive["execution"], "chigiri_mock");
    }
    let representation = directives
        .iter()
        .find(|directive| directive["name"] == "representation")
        .unwrap();
    assert_eq!(representation["locations"], json!(["SCALAR"]));
    assert_eq!(representation["arguments"][0]["name"], "type");
    assert_eq!(
        representation["arguments"][0]["value_type"],
        "ChigiriMockRepresentation!"
    );
    assert_eq!(representation["arguments"][0]["required"], true);
    assert_eq!(
        representation["arguments"][0]["choices"],
        json!([
            "STRING", "BOOLEAN", "INTEGER", "NUMBER", "OBJECT", "ARRAY", "JSON"
        ])
    );
    let constraint = directives
        .iter()
        .find(|directive| directive["name"] == "constraint")
        .unwrap();
    assert_eq!(
        constraint["locations"],
        json!([
            "FIELD_DEFINITION",
            "INPUT_FIELD_DEFINITION",
            "ARGUMENT_DEFINITION"
        ])
    );
    assert_eq!(constraint["arguments"].as_array().unwrap().len(), 15);
    let format = constraint["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|argument| argument["name"] == "format")
        .unwrap();
    assert_eq!(
        format["choices"],
        json!([
            "byte",
            "date-time",
            "date",
            "email",
            "ipv4",
            "ipv6",
            "uri",
            "uuid"
        ])
    );
    let relation = directives
        .iter()
        .find(|directive| directive["name"] == "relation")
        .unwrap();
    assert_eq!(relation["locations"], json!(["FIELD_DEFINITION"]));
    assert_eq!(
        relation["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|argument| {
                (
                    argument["name"].as_str().unwrap(),
                    argument["value_type"].as_str().unwrap(),
                    argument["required"].as_bool().unwrap(),
                )
            })
            .collect::<Vec<_>>(),
        [
            ("fields", "[String!]", false),
            ("references", "[String!]", false),
            ("name", "String", false),
        ]
    );
    let virtual_directive = directives
        .iter()
        .find(|directive| directive["name"] == "virtual")
        .unwrap();
    assert_eq!(virtual_directive["locations"], json!(["FIELD_DEFINITION"]));
    assert!(
        virtual_directive["arguments"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(representation["arguments"].as_array().unwrap().len(), 7);
    let deprecated = directives
        .iter()
        .find(|directive| directive["name"] == "deprecated")
        .unwrap();
    assert_eq!(
        deprecated["arguments"][0]["default"],
        json!({"kind":"string", "value":"No longer supported"})
    );
    let _: ReferenceDocument = serde_json::from_value(wire).unwrap();
}

#[test]
fn ref_05_each_mock_directive_has_independent_ir_positive_and_negative_examples() {
    let wire = value(&ReferenceDocument::build(None).unwrap());
    for directive in wire["directives"].as_array().unwrap().iter().skip(5) {
        let examples = directive["examples"].as_array().unwrap();
        assert_eq!(examples.len(), 2, "{}", directive["name"]);
        for example in examples {
            let result = MockIr::parse(vec![Source::new(
                "directive-example.graphql",
                example["sdl"].as_str().unwrap(),
            )]);
            if example["valid"] == true {
                assert!(result.is_ok(), "{}", example["id"]);
            } else {
                let error = result.err().unwrap_or_else(|| {
                    panic!("negative example must be rejected: {}", example["id"])
                });
                assert_eq!(error.code, example["expected"], "{}", example["id"]);
            }
        }
    }
}

#[test]
fn ref_06_user_directive_is_declared_only_and_keeps_description_origin() {
    let ir = MockIr::parse(vec![Source::new(
        "custom.graphql",
        "\"For auditing\" directive @audit(level: Int! = 1, tags: [String!]! = [\"stable\"]) repeatable on FIELD_DEFINITION\ntype Query { value: String @audit }",
    )])
    .unwrap();
    let wire = value(&ReferenceDocument::build(Some(&ir)).unwrap());
    let audit = wire["directives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|directive| directive["name"] == "audit")
        .unwrap();
    assert_eq!(audit["class"], "user_declared");
    assert_eq!(audit["execution"], "declaration_only");
    assert_eq!(audit["declared_description"]["origin"], "user_sdl");
    assert_eq!(audit["declared_description"]["text"], "For auditing");
    assert_eq!(audit["arguments"][0]["required"], false);
    assert_eq!(
        audit["arguments"][0]["default"],
        json!({"kind":"integer", "value":"1"})
    );
    assert!(audit["syntax"].as_str().unwrap().contains("[\"stable\"]"));
    assert!(
        audit["purpose"]
            .as_str()
            .unwrap()
            .contains("no Mock execution semantics")
    );
}

#[test]
fn ref_09_directive_wire_is_closed_and_old_v5_is_rejected() {
    let document = ReferenceDocument::build(None).unwrap();
    let original = value(&document);
    for pointer in ["/directives/0", "/directives/0/arguments/0"] {
        let mut changed = original.clone();
        changed
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(true));
        assert!(serde_json::from_value::<ReferenceDocument>(changed).is_err());
    }
    let mut old = original;
    old["format"] = json!("chigiri-mock-reference/v5");
    assert!(serde_json::from_value::<ReferenceDocument>(old).is_err());
}
