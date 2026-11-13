//! Compare the shared closed graph and its single or repeated ID references with independent wire expectations.

use super::{Format, ReferenceDocument};
use crate::ir::MockIr;
use chigiri::core::Source;
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn visit(value: &Value, f: &mut impl FnMut(&serde_json::Map<String, Value>)) {
    match value {
        Value::Object(object) => {
            f(object);
            for value in object.values() {
                visit(value, f);
            }
        }
        Value::Array(values) => {
            for value in values {
                visit(value, f);
            }
        }
        _ => {}
    }
}

pub(super) fn graph_is_closed(value: &Value) -> bool {
    let mut ids = BTreeSet::new();
    let mut valid = true;
    visit(value, &mut |object| {
        if let Some(id) = object.get("id").and_then(Value::as_str) {
            ids.insert(id.to_owned());
        }
        // Require unique IDs within each entity array, such as rules or namespaces.
        for values in object.values().filter_map(Value::as_array) {
            let mut local = BTreeSet::new();
            for id in values.iter().filter_map(|value| value["id"].as_str()) {
                valid &= local.insert(id);
            }
        }
    });
    visit(value, &mut |object| {
        for (name, value) in object {
            if name.ends_with("_id") {
                if let Some(id) = value.as_str() {
                    valid &= ids.contains(id);
                }
            } else if name.ends_with("_ids") {
                for id in value.as_array().unwrap() {
                    valid &= ids.contains(id.as_str().unwrap());
                }
            }
        }
    });
    let types: BTreeSet<_> = value["api_types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    for method in value["methods"].as_array().unwrap() {
        valid &= types.contains(method["returns"].as_str().unwrap());
        for arg in method["arguments"].as_array().unwrap() {
            valid &= types.contains(arg["value_type"].as_str().unwrap());
        }
    }
    for option in value["options"].as_array().unwrap() {
        valid &= types.contains(option["value_type"].as_str().unwrap());
    }
    valid
}

#[test]
fn mr_01_02_17_20_common_type_shapes_and_all_reference_ids_resolve_without_prose() {
    let ir = MockIr::parse(vec![Source::new(
        "graph.graphql",
        include_str!("../../fixtures/projects.graphql"),
    )])
    .unwrap();
    for ir in [None, Some(&ir)] {
        let doc = ReferenceDocument::build(ir).unwrap();
        let value: Value = serde_json::from_slice(&doc.render(Format::Json).unwrap()).unwrap();
        assert!(graph_is_closed(&value));
        let types = value["api_types"].as_array().unwrap();
        let shape = |id| &types.iter().find(|row| row["id"] == id).unwrap()["shape"];
        assert_eq!(
            shape("predicate"),
            &json!({"kind":"union", "variant_type_ids":["predicate_all","predicate_any","predicate_leaf"]})
        );
        for (id, fields) in [
            ("predicate_all", vec![("all", "predicates")]),
            ("predicate_any", vec![("any", "predicates")]),
            (
                "predicate_leaf",
                vec![
                    ("field", "field_name"),
                    ("op", "operator"),
                    ("value", "operand"),
                ],
            ),
            (
                "sort_key",
                vec![("field", "field_name"), ("direction", "direction")],
            ),
        ] {
            let expected: Vec<_> = fields
                .iter()
                .map(
                    |(name, ty)| json!({"name":name,"type_id":ty,"required":true,"nullable":false}),
                )
                .collect();
            assert_eq!(
                shape(id),
                &json!({"kind":"object","closed":true,"fields":expected})
            );
        }
        assert_eq!(
            shape("sort_keys"),
            &json!({"kind":"array","item_type_id":"sort_key","dense":true,"maximum":4})
        );
        assert_eq!(shape("direction")["values"], json!(["asc", "desc"]));
        assert_eq!(shape("operand")["nullable"], false);
        assert_eq!(shape("operand")["apply_storage_constraints"], false);
        for (id, include_key, omission, null_policy, typename) in [
            (
                "record",
                true,
                "nullable_slots",
                "storage_nullability",
                true,
            ),
            (
                "create_input",
                true,
                "nullable_slots",
                "storage_nullability",
                false,
            ),
            ("update_input", false, "all_slots", "all_slots", false),
        ] {
            assert_eq!(
                shape(id)["value_policy"],
                json!({"include_key":include_key,"omission":omission,
                    "top_level_null":null_policy,"validate_storage_constraints":true,"required_model_typename":typename})
            );
            assert_eq!(
                shape(id)["typename_field_name"],
                if typename {
                    json!("__typename")
                } else {
                    Value::Null
                }
            );
        }
        let members = shape("context")["fields"].as_array().unwrap();
        assert_eq!(
            members
                .iter()
                .map(|row| row["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "scope",
                "null",
                "models",
                "state",
                "jsonObject",
                "jsonArray"
            ]
        );
        assert!(
            members
                .iter()
                .all(|row| row["required"] == true && row["nullable"] == false)
        );
        assert_eq!(
            shape("null_sentinel"),
            &json!({"kind":"null_sentinel","lua_nil":false})
        );
        assert_eq!(
            shape("context_scope")["values"],
            json!(["read", "mutation"])
        );
        for (id, ty, marker) in [
            ("json_object_constructor", "json_object", "object"),
            ("json_array_constructor", "json_array", "array"),
        ] {
            assert_eq!(
                shape(id),
                &json!({"kind":"function","calling_convention":"dot","arity":1,
                "argument_type_ids":[ty],"returns_type_id":ty,"returns_same_table":true,"json_marker":marker})
            );
        }
        for context in value["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| !row["id"].as_str().unwrap().starts_with("hook/"))
        {
            assert_eq!(context["type_id"], "context");
            assert_eq!(
                context["scope_value"],
                if context["id"] == "mutation_root" {
                    "mutation"
                } else {
                    "read"
                }
            );
        }
        for (id, expected) in [
            ("record", "record_slot_ids"),
            ("create_input", "create_slot_ids"),
            ("update_input", "update_slot_ids"),
        ] {
            assert_eq!(shape(id)["binding"], expected);
        }
        let _: ReferenceDocument = serde_json::from_value(value.clone()).unwrap();
        let mut bad = value.clone();
        bad["api_types"][0]["shape"]["unknown"] = json!(true);
        assert!(serde_json::from_value::<ReferenceDocument>(bad).is_err());
        for (pointer, replacement) in [
            ("/methods/0/returns", json!("missing_type")),
            ("/methods/0/rule_ids/0", json!("missing_rule")),
            ("/api_types/0/id", json!("predicate")),
        ] {
            let mut bad = value.clone();
            *bad.pointer_mut(pointer).unwrap() = replacement;
            assert!(!graph_is_closed(&bad), "{pointer}");
        }
        if ir.is_some() {
            for pointer in [
                "/schema/types/0/field_ids",
                "/schema/models/0/api_types/record_slot_ids",
            ] {
                let mut bad = value.clone();
                *bad.pointer_mut(pointer).unwrap() = json!(["missing_schema_id"]);
                assert!(!graph_is_closed(&bad));
            }
        }
        let limits = value["limits"].as_array().unwrap();
        for (id, scope, maximum) in [
            ("value.items", "list_or_json_array", 1000),
            ("store.nodes", "store", 100_000),
            ("store.bytes", "store", 16_777_216),
        ] {
            let row = limits.iter().find(|row| row["id"] == id).unwrap();
            assert_eq!(row["shared_scope"], scope);
            assert_eq!(row["value"], maximum);
        }
    }
}
