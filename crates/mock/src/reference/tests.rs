//! Verifies closed wire formats, ID references, exact scope types,
//! deterministic output, and the upper resource boundary.

use super::{Format, ReferenceDocument};
use crate::ir::MockIr;
use chigiri::core::Source;
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn document(sdl: &str) -> ReferenceDocument {
    let ir = MockIr::parse(vec![Source::new("logical.graphql", sdl)]).unwrap();
    ReferenceDocument::build(Some(&ir)).unwrap()
}

fn wire(doc: &ReferenceDocument) -> Value {
    serde_json::from_slice(&doc.render(Format::Json).unwrap()).unwrap()
}

#[test]
fn mr_01_16_18_common_document_is_closed_complete_and_deterministic() {
    let doc = ReferenceDocument::build(None).unwrap();
    let bytes = doc.render(Format::Json).unwrap();
    assert_eq!(
        bytes,
        ReferenceDocument::build(None)
            .unwrap()
            .render(Format::Json)
            .unwrap()
    );
    let value = wire(&doc);
    assert_eq!(value["format"], "chigiri-mock-reference/v13");
    assert_eq!(value["api_version"], "chigiri-mock-api/v10");
    assert!(value["mock_format"].is_null());
    assert_eq!(value["input_kind"], "common");
    assert_eq!(value["config_executed"], false);
    assert_eq!(value["supported_formats"]["bundle"], "chigiri-mock/v11");
    assert_eq!(
        value["supported_formats"]["project"],
        "chigiri-mock-project/v10"
    );
    assert!(value.get("config_format").is_none());
    assert_eq!(value["readiness_checked"], false);
    assert!(value["schema"].is_null());
    let names: BTreeSet<_> = value["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        BTreeSet::from([
            "find",
            "create",
            "update",
            "delete",
            "count",
            "connect",
            "disconnect"
        ])
    );
    for method in value["methods"].as_array().unwrap() {
        assert_eq!(
            method["arity_including_receiver"].as_u64().unwrap() as usize,
            method["arguments"].as_array().unwrap().len() + 1
        );
        for field in ["rule_ids", "example_ids"] {
            let target = if field == "rule_ids" {
                "rules"
            } else {
                "examples"
            };
            for id in method[field].as_array().unwrap() {
                assert!(
                    value[target]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|row| &row["id"] == id)
                );
            }
        }
    }
    let _: ReferenceDocument = serde_json::from_slice(&bytes).unwrap();
    for pointer in [
        "",
        "/methods/0",
        "/methods/0/arguments/0",
        "/options/0",
        "/operators/0",
        "/rules/0",
        "/examples/0",
        "/limits/0",
        "/contexts/0",
        "/binary",
    ] {
        let mut bad = value.clone();
        bad.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(true));
        assert!(
            serde_json::from_value::<ReferenceDocument>(bad).is_err(),
            "{pointer}"
        );
    }
    let text = String::from_utf8(doc.render(Format::Text).unwrap()).unwrap();
    for name in names {
        assert!(text.contains(&format!(":{name}(")));
    }
    assert!(text.contains("generic_illustration"));
    assert!(text.contains("readiness_checked: false"));
}

#[test]
fn mr_17_schema_graph_preserves_wrappers_defaults_relations_and_scope() {
    let doc = document(
        r#"
        schema { query: Read mutation: Write }
        "Description\u001b\u0085\u202e" type Person {
            id: ID!
            note: String
            edgeIds: [ID]
            end: [Person] @relation(fields:["edgeIds"], references:["id"])
            back(limit:Int! = 20): [Person!]!
        }
        input Pick @oneOf { id:ID name:String }
        input Input { "Default value" count:Int! = 7 pick:Pick data:[[Int!]!] }
        type Tree { label:String! children:[Tree!]! }
        union Roots = Read | Write
        type Read { root:Roots! @virtual tree(input:Input):Tree }
        type Write { edit(id:ID!): Person }
    "#,
    );
    let value = wire(&doc);
    let schema = &value["schema"];
    let _: ReferenceDocument = serde_json::from_value(value.clone()).unwrap();
    let types = schema["types"].as_array().unwrap();
    let fields = schema["fields"].as_array().unwrap();
    let ids: BTreeSet<_> = [
        "types",
        "fields",
        "inputs",
        "models",
        "slots",
        "relations",
        "resolvers",
        "capabilities",
    ]
    .iter()
    .flat_map(|name| schema[name].as_array().unwrap())
    .map(|row| row["id"].as_str().unwrap())
    .collect();
    fn references(value: &Value, ids: &BTreeSet<&str>) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    if key.ends_with("_id")
                        && key != "method_id"
                        && let Some(value) = value.as_str()
                    {
                        assert!(ids.contains(value), "{key}: {value}");
                    }
                    references(value, ids);
                }
            }
            Value::Array(values) => {
                for value in values {
                    references(value, ids);
                }
            }
            _ => {}
        }
    }
    references(schema, &ids);
    let person = types.iter().find(|ty| ty["name"] == "Person").unwrap();
    assert_eq!(person["description"]["origin"], "user_sdl");
    assert_eq!(
        person["description"]["text"],
        "Description\u{1b}\u{85}\u{202e}"
    );
    let pick = types.iter().find(|ty| ty["name"] == "Pick").unwrap();
    assert_eq!(pick["one_of"], true);
    let data = schema["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|input| input["name"] == "data")
        .unwrap();
    assert_eq!(
        data["value_type"]["wrappers"],
        json!(["list", "non_null", "list", "non_null"])
    );
    let count = schema["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|input| input["name"] == "count")
        .unwrap();
    assert_eq!(count["default"], json!({"kind":"integer", "value":"7"}));
    assert_eq!(count["required_after_coercion"], true);
    let relation = &schema["relations"][0];
    assert_eq!(relation["many"], true);
    assert_eq!(relation["nullable_items"], true);
    let owner = fields
        .iter()
        .find(|field| field["id"] == relation["owner_field_id"])
        .unwrap();
    assert_eq!(owner["name"], "end");
    assert_eq!(
        schema["capabilities"][0]["models"][0]["method_ids"],
        json!(["model.count", "model.find"])
    );
    let text = String::from_utf8(doc.render(Format::Text).unwrap()).unwrap();
    assert!(
        !text
            .chars()
            .any(|ch| matches!(ch, '\u{1b}' | '\u{85}' | '\u{202e}'))
    );
    assert!(text.contains("user_sdl description:"));
    assert!(text.contains("\\u{1b}\\u{85}\\u{202e}"));
    assert!(
        value["examples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|example| example["origin"] == "schema_specific"
                && example["luau"] == "return ctx.models[\"Person\"]:count({})")
    );
}

#[test]
fn subscription_resolvers_are_not_reported_as_mutation_capabilities() {
    let doc = document(
        "schema{query:Query mutation:Mutation subscription:Subscription} type Query{x:Int} type Mutation{x:Int} type Subscription{tick:Int}",
    );
    let value = wire(&doc);
    let resolver = value["schema"]["resolvers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|resolver| resolver["coordinate"] == "Subscription.tick")
        .unwrap();
    assert_eq!(
        resolver["scopes"],
        serde_json::json!([
            {"scope":"subscription_setup", "capability_id":"capability/read"}
        ])
    );
    assert!(
        value["schema"]["models"]
            .as_array()
            .unwrap()
            .iter()
            .all(|model| model["name"] != "Subscription")
    );
}

#[test]
fn mr_17_18_21_large_repeated_coordinate_stops_during_document_construction() {
    let name = "X".repeat(90_000);
    let fields: String = (0..255)
        .map(|index| format!("f{index}:Int @virtual\n"))
        .collect();
    let sdl = format!("type {name} {{ {fields} }} type Query {{ probe:Boolean }}");
    assert!(sdl.len() < crate::schema::MAX_SDL_BYTES);
    let ir = MockIr::parse(vec![Source::new("private-path", sdl)]).unwrap();
    let error = match ReferenceDocument::build(Some(&ir)) {
        Ok(_) => panic!("overcapacity must be detected"),
        Err(error) => error,
    };
    assert_eq!(error.to_string(), "reference exceeds 16 MiB");
    assert!(!error.to_string().contains("private-path"));
}
