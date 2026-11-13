//! Detect intentional inconsistencies against fixed model-runtime contracts using independent expectations.

use super::{Format, ReferenceDocument};
use serde_json::{Value, json};

fn conforms(doc: &Value) -> bool {
    let expected = [
        (
            "count",
            "read",
            "all",
            vec!["count_options"],
            "number",
            "count.exact",
            "count.records",
            "query",
        ),
        (
            "find",
            "read",
            "all",
            vec!["find_options"],
            "records",
            "find.snapshot",
            "find.records",
            "query",
        ),
        (
            "create",
            "write",
            "all",
            vec!["create_input"],
            "record",
            "create.generated-id",
            "create.record",
            "storage",
        ),
        (
            "update",
            "write",
            "all",
            vec!["find_options", "update_input"],
            "records",
            "update.outer-patch",
            "update.patch",
            "storage",
        ),
        (
            "delete",
            "write",
            "all",
            vec!["find_options"],
            "number",
            "delete.restrict",
            "delete.record",
            "relation",
        ),
        (
            "connect",
            "write",
            "relations",
            vec!["find_options", "relation", "target_options"],
            "records",
            "connect.owner",
            "connect.owner",
            "relation",
        ),
        (
            "disconnect",
            "write",
            "relations",
            vec!["find_options", "relation", "target_options"],
            "records",
            "disconnect.owner",
            "disconnect.owner",
            "relation",
        ),
    ];
    let methods = doc["methods"].as_array().unwrap();
    if methods.len() != expected.len() {
        return false;
    }
    for (name, effect, eligibility, args, returns, rule, example, failure) in expected {
        let matches: Vec<_> = methods.iter().filter(|row| row["name"] == name).collect();
        if matches.len() != 1 {
            return false;
        }
        let method = matches[0];
        if method["id"] != format!("model.{name}")
            || method["effect"] != effect
            || method["model_eligibility"] != eligibility
            || method["returns"] != returns
            || method["arity_including_receiver"] != args.len() + 1
            || method["rule_ids"] != json!([rule])
            || method["example_ids"] != json!([example])
        {
            return false;
        }
        let actual = method["arguments"].as_array().unwrap();
        if actual.len() != args.len()
            || actual
                .iter()
                .zip(args)
                .any(|(arg, expected)| arg["value_type"] != expected || arg["required"] != true)
        {
            return false;
        }
        let Some(rule) = doc["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == rule)
        else {
            return false;
        };
        if rule["failure_class"] != failure || rule["example_ids"] != json!([example]) {
            return false;
        }
    }
    for context in doc["contexts"].as_array().unwrap() {
        if matches!(
            context["id"].as_str(),
            Some(
                "query_root"
                    | "nested"
                    | "hook/request.before"
                    | "hook/request.after"
                    | "hook/operation.before"
                    | "hook/operation.after"
                    | "hook/field.before"
                    | "hook/field.after"
            )
        ) {
            if context["effect"] != "read"
                || context["method_ids"] != json!(["model.count", "model.find"])
            {
                return false;
            }
        } else if context["id"] != "mutation_root"
            || context["effect"] != "write"
            || context["method_ids"].as_array().unwrap().len() != 7
        {
            return false;
        }
    }
    let options = doc["options"].as_array().unwrap();
    if options.len() != 5 {
        return false;
    }
    for (id, nullable, default, maximum) in [
        ("model.find.where", true, Value::Null, Value::Null),
        ("model.find.orderBy", false, Value::Null, json!(4)),
        (
            "model.find.offset",
            false,
            json!(0),
            json!(9_007_199_254_740_991_u64),
        ),
        (
            "model.find.limit",
            false,
            Value::Null,
            json!(9_007_199_254_740_991_u64),
        ),
        ("model.count.where", true, Value::Null, Value::Null),
    ] {
        let Some(option) = options.iter().find(|row| row["id"] == id) else {
            return false;
        };
        if option["nullable"] != nullable
            || option["default"] != default
            || option["maximum"] != maximum
        {
            return false;
        }
    }
    true
}

#[test]
fn mr_02_metadata_mutations_detect_method_scope_type_default_limit_rule_and_example_drift() {
    let doc = ReferenceDocument::build(None).unwrap();
    let value: Value = serde_json::from_slice(&doc.render(Format::Json).unwrap()).unwrap();
    assert!(conforms(&value));
    let mut missing = value.clone();
    missing["methods"].as_array_mut().unwrap().remove(0);
    assert!(!conforms(&missing));
    let mut duplicate = value.clone();
    duplicate["methods"]
        .as_array_mut()
        .unwrap()
        .push(value["methods"][0].clone());
    assert!(!conforms(&duplicate));
    for (pointer, replacement) in [
        ("/methods/0/effect", json!("write")),
        (
            "/contexts/0/method_ids",
            json!(["model.count", "model.find", "model.create"]),
        ),
        ("/methods/0/arguments/0/value_type", json!("find_options")),
        ("/methods/0/returns", json!("records")),
        ("/options/3/default", json!(21)),
        ("/options/3/maximum", json!(101)),
        ("/methods/0/rule_ids", json!(["find.snapshot"])),
        ("/methods/0/example_ids", json!(["find.records"])),
        ("/rules/0/failure_class", json!("storage")),
    ] {
        let mut broken = value.clone();
        *broken.pointer_mut(pointer).unwrap() = replacement;
        assert!(!conforms(&broken), "{pointer}");
    }
    let mut unknown_failure = value;
    unknown_failure["rules"][0]["failure_class"] = json!("unknown");
    assert!(serde_json::from_value::<ReferenceDocument>(unknown_failure).is_err());
}
