//! Compares the API oracle and all constraint rules independently of the common
//! registry.

use chigiri::core::Source;
use serde_json::{Value, json};

use super::{Format, ReferenceDocument};
use crate::ir::MockIr;

#[test]
fn sd_api_reference_has_seven_selection_signatures_and_both_relation_directions() {
    let ir = MockIr::parse(vec![Source::new("reference.graphql", r#"
        type Parent { code: Int! children: [Child!]! @relation }
        type Child { value: String! parent: Parent @relation(fields:["parentCode"], references:["code"]) }
        type Query { parents: [Parent!]! }
    "#)]).unwrap();
    let doc = ReferenceDocument::build(Some(&ir)).unwrap();
    let value: Value = serde_json::from_slice(&doc.render(Format::Json).unwrap()).unwrap();
    let expected = [
        ("find", vec!["find_options"], "records"),
        ("count", vec!["count_options"], "number"),
        ("create", vec!["create_input"], "record"),
        ("update", vec!["find_options", "update_input"], "records"),
        ("delete", vec!["find_options"], "number"),
        (
            "connect",
            vec!["find_options", "relation", "target_options"],
            "records",
        ),
        (
            "disconnect",
            vec!["find_options", "relation", "target_options"],
            "records",
        ),
    ];
    let methods = value["methods"].as_array().unwrap();
    assert_eq!(methods.len(), expected.len());
    for (name, arguments, returns) in expected {
        let method = methods
            .iter()
            .find(|method| method["name"] == name)
            .unwrap();
        assert_eq!(method["returns"], returns);
        let actual: Vec<_> = method["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|argument| argument["value_type"].as_str().unwrap())
            .collect();
        assert_eq!(actual, arguments);
    }
    for model in value["schema"]["models"].as_array().unwrap() {
        assert_eq!(model["method_ids"].as_array().unwrap().len(), 7);
        assert_eq!(
            model["api_types"]["relation_field_ids"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    let options = value["options"].as_array().unwrap();
    let limit = options
        .iter()
        .find(|option| option["id"] == "model.find.limit")
        .unwrap();
    assert!(limit["default"].is_null());
    assert_eq!(limit["maximum"], json!(9_007_199_254_740_991_u64));
    let records = value["api_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|ty| ty["id"] == "records")
        .unwrap();
    assert_eq!(records["shape"]["maximum"], 1000);
    let text = String::from_utf8(doc.render(Format::Text).unwrap()).unwrap();
    assert!(text.contains("Target<M, relation>"));
    assert!(text.contains("Relation<M>"));
}

#[test]
fn sd_api_reference_keeps_all_fifteen_constraint_arguments_and_closed_formats() {
    let ir = MockIr::parse(vec![Source::new("constraints.graphql", r#"
        type Data {
            text: [String!]! @constraint(minLength:1,maxLength:40,startsWith:"a",endsWith:"z",contains:"b",notContains:"x",pattern:"^a.*z$",format:"email",minItems:1,maxItems:10)
            number: Float! @constraint(min:0,max:10,exclusiveMin:0,exclusiveMax:10,multipleOf:0.5)
        }
        type Query { data: Data }
    "#)]).unwrap();
    let doc = ReferenceDocument::build(Some(&ir)).unwrap();
    let value: Value = serde_json::from_slice(&doc.render(Format::Json).unwrap()).unwrap();
    let fields = value["schema"]["fields"].as_array().unwrap();
    let text = &fields.iter().find(|field| field["name"] == "text").unwrap()["constraints"];
    for (name, expected) in [
        ("min_length", json!(1)),
        ("max_length", json!(40)),
        ("starts_with", json!("a")),
        ("ends_with", json!("z")),
        ("contains", json!("b")),
        ("not_contains", json!("x")),
        ("pattern", json!("^a.*z$")),
        ("format", json!("email")),
        ("min_items", json!(1)),
        ("max_items", json!(10)),
    ] {
        assert_eq!(text[name], expected, "{name}");
    }
    let number = &fields
        .iter()
        .find(|field| field["name"] == "number")
        .unwrap()["constraints"];
    for (name, expected) in [
        ("min", 0.0),
        ("max", 10.0),
        ("exclusive_min", 0.0),
        ("exclusive_max", 10.0),
        ("multiple_of", 0.5),
    ] {
        assert_eq!(number[name], json!(expected), "{name}");
    }
    let mut bad = value.clone();
    for field in bad["schema"]["fields"].as_array_mut().unwrap() {
        if field["name"] == "text" {
            field["constraints"]["format"] = json!("unknown");
        }
    }
    assert!(serde_json::from_value::<ReferenceDocument>(bad).is_err());
    assert!(serde_json::from_value::<ReferenceDocument>(value).is_ok());
}

#[test]
fn sd_constraint_meaning_and_actual_regex_limits_exist_without_schema_in_both_formats() {
    let document = super::ReferenceDocument::build(None).unwrap();
    let wire: serde_json::Value =
        serde_json::from_slice(&document.render(super::Format::Json).unwrap()).unwrap();
    let text = String::from_utf8(document.render(super::Format::Text).unwrap()).unwrap();
    for (id, expected) in [
        ("constraint.locations", "uniqueTypeName"),
        ("constraint.unicode", "U+FE0F"),
        ("constraint.wrappers", "outermost list"),
        ("constraint.multiple_of", "0.30000000000000004"),
        ("constraint.formats", "mailto:"),
        ("constraint.regex_dialect", "(a)\\1"),
        ("constraint.regex_resources", "process RSS"),
    ] {
        let rule = wire["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|rule| rule["id"] == id)
            .unwrap();
        assert!(
            rule["description"].as_str().unwrap().contains(expected),
            "{id}"
        );
        assert!(text.contains(expected), "{id}");
    }
    for (id, expected) in [
        ("regex.matches", 4096),
        ("regex.compiled_bytes", 4 * 1024 * 1024),
        ("regex.schema_cache_bytes", 16 * 1024 * 1024),
        ("regex.match_bytes", 1024 * 1024),
    ] {
        let limit = wire["limits"]
            .as_array()
            .unwrap()
            .iter()
            .find(|limit| limit["id"] == id)
            .unwrap();
        assert_eq!(limit["value"], expected);
        assert!(text.contains(id));
    }
}
