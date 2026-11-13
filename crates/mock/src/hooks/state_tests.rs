//! Test state declarations and values independently, including atomic replacement, shared budgets, and request isolation.

use chigiri::core::{Object, Value};
use serde_json::json;

use super::{HookSlot, StateContract, StateFields, StateType};
use crate::value::{MAX_ITEMS, MAX_NODES, MAX_STRING_BYTES, ValueBudget};

fn value(json: serde_json::Value) -> Value {
    chigiri_http::value_from_json(json).unwrap()
}

fn contract(fields: serde_json::Value, initial: serde_json::Value) -> StateContract {
    StateContract::parse(&value(fields), &value(initial), &mut ValueBudget::default()).unwrap()
}

#[test]
fn hk_02_all_declared_kinds_normalize_nullable_without_guessing_empty_shapes() {
    let contract = contract(
        json!({
            "text": {"kind": "string"},
            "flag": {"kind": "boolean"},
            "count": {"kind": "integer"},
            "ratio": {"kind": "number"},
            "nested": {"kind": "object", "fields": {
                "optional": {"kind": "nullable", "of": {"kind": "string"}}
            }},
            "items": {"kind": "list", "of": {"kind": "boolean"}},
            "optional": {"kind": "nullable", "of": {"kind": "object", "fields": {}}}
        }),
        json!({"text":"résumé 🙂", "flag":false, "count":-9007199254740991_i64,
               "ratio":0.25, "nested":{}, "items":[]}),
    );
    let snapshot = contract.instantiate().snapshot();
    assert_eq!(
        snapshot.get("text"),
        Some(&Value::String("résumé 🙂".into()))
    );
    assert_eq!(snapshot.get("optional"), Some(&Value::Null));
    assert_eq!(snapshot.get("items"), Some(&Value::List(vec![])));
    assert_eq!(
        snapshot.get("nested"),
        Some(&value(json!({"optional": null})))
    );
    assert_eq!(
        snapshot.get("count"),
        Some(&Value::Int(-9_007_199_254_740_991))
    );
    assert!(matches!(contract.fields()["items"], StateType::List { .. }));
}

#[test]
fn hk_02_closed_kind_and_field_declarations_reject_unknown_or_reserved_shapes() {
    for fields in [
        json!([]),
        json!({"x": {"kind":"any"}}),
        json!({"x": {"kind":"string", "of":{"kind":"string"}}}),
        json!({"x": {"kind":"list"}}),
        json!({"x": {"kind":"object", "fields":[], "extra":true}}),
        json!({"x": {"kind":"nullable", "of":{"kind":"string"}, "fields":{}}}),
        json!({"x": "number"}),
        json!({"x": {}}),
        json!({"": {"kind":"boolean"}}),
        json!({"bad-name": {"kind":"boolean"}}),
        json!({"__typename": {"kind":"boolean"}}),
        json!({"Name": {"kind":"boolean"}, "name": {"kind":"boolean"}}),
    ] {
        assert!(
            StateContract::parse(
                &value(fields.clone()),
                &value(json!({})),
                &mut ValueBudget::default()
            )
            .is_err(),
            "{fields}"
        );
    }
    // Preserve keywords as quoted keys, matching the existing generator.
    assert!(
        contract(json!({"end":{"kind":"boolean"}}), json!({"end":true}))
            .fields()
            .contains_key("end")
    );
}

#[test]
fn hk_02_required_type_unknown_and_nonfinite_initial_values_are_rejected() {
    let fields = value(json!({"x":{"kind":"integer"}}));
    for initial in [
        json!({}),
        json!({"x":null}),
        json!({"x":"1"}),
        json!({"x":true}),
        json!({"x":1.25}),
        json!({"x":9007199254740992_i64}),
        json!({"x":-9007199254740992_i64}),
        json!({"x":1,"extra":false}),
    ] {
        assert!(
            StateContract::parse(
                &fields,
                &value(initial.clone()),
                &mut ValueBudget::default()
            )
            .is_err(),
            "{initial}"
        );
    }
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for ty in [StateType::Integer, StateType::Number] {
            let fields = StateFields::from([("x".into(), ty)]);
            let initial = Value::Object(Object::from_iter([("x", Value::Float(number))]));
            assert!(StateContract::checked(fields, &initial, &mut ValueBudget::default()).is_err());
        }
    }
    let contract = contract(json!({"x":{"kind":"integer"}}), json!({"x":1.0}));
    assert_eq!(contract.initial().get("x"), Some(&Value::Int(1)));
}

#[test]
fn hk_04_patch_is_top_level_replacement_atomic_and_request_local() {
    let contract = contract(
        json!({"authenticated":{"kind":"boolean"},
        "user_id":{"kind":"nullable","of":{"kind":"string"}},
        "profile":{"kind":"object","fields": {
            "name":{"kind":"string"}, "active":{"kind":"boolean"}
        }}}),
        json!({"authenticated":false,"profile":{"name":"old","active":true}}),
    );
    let mut first = contract.instantiate();
    let second = contract.instantiate();
    let original = first.snapshot();
    first
        .apply_patch(
            HookSlot::RequestBefore,
            &value(json!({"authenticated":true,"user_id":"u1"})),
            &mut ValueBudget::default(),
        )
        .unwrap();
    let authenticated = first.snapshot();
    assert_eq!(
        authenticated.get("authenticated"),
        Some(&Value::Boolean(true))
    );
    assert_eq!(authenticated.get("profile"), original.get("profile"));
    assert_eq!(*second.snapshot(), *original);
    assert_eq!(contract.initial(), original.as_ref());

    for patch in [
        json!({"authenticated":false,"profile":{"name":"new"}}),
        json!({"authenticated":false,"user_id":7}),
        json!({"authenticated":false,"unknown":true}),
    ] {
        assert!(
            first
                .apply_patch(
                    HookSlot::OperationBefore,
                    &value(patch),
                    &mut ValueBudget::default()
                )
                .is_err()
        );
        assert!(std::sync::Arc::ptr_eq(&authenticated, &first.snapshot()));
    }
    first
        .apply_patch(
            HookSlot::OperationBefore,
            &value(json!({"user_id":null,"profile":{"name":"new","active":false}})),
            &mut ValueBudget::default(),
        )
        .unwrap();
    assert_eq!(first.snapshot().get("user_id"), Some(&Value::Null));
    assert_eq!(
        first.snapshot().get("profile"),
        Some(&value(json!({"name":"new","active":false})))
    );
    assert_eq!(
        authenticated.get("user_id"),
        Some(&Value::String("u1".into()))
    );
}

#[test]
fn hk_03_field_and_after_cannot_apply_even_an_empty_patch() {
    let mut state = StateContract::default().instantiate();
    for slot in HookSlot::ALL {
        let original = state.snapshot();
        let result = state.apply_patch(
            slot,
            &Value::Object(Object::new()),
            &mut ValueBudget::default(),
        );
        assert_eq!(result.is_ok(), slot.allows_patch(), "{}", slot.name());
        if !slot.allows_patch() {
            assert!(std::sync::Arc::ptr_eq(&original, &state.snapshot()));
        }
    }
}

#[test]
fn hk_02_04_string_list_depth_and_shared_node_budgets_remain_bounded() {
    for count in [MAX_ITEMS, MAX_ITEMS + 1] {
        let fields = StateFields::from([(
            "items".into(),
            StateType::List {
                of: Box::new(StateType::Boolean),
            },
        )]);
        let initial = Value::Object(Object::from_iter([(
            "items",
            Value::List(vec![Value::Boolean(true); count]),
        )]));
        assert_eq!(
            StateContract::checked(fields, &initial, &mut ValueBudget::default()).is_ok(),
            count == MAX_ITEMS
        );
    }
    for count in [MAX_STRING_BYTES, MAX_STRING_BYTES + 1] {
        let fields = StateFields::from([("text".into(), StateType::String)]);
        let initial = Value::Object(Object::from_iter([(
            "text",
            Value::String("x".repeat(count)),
        )]));
        assert_eq!(
            StateContract::checked(fields, &initial, &mut ValueBudget::default()).is_ok(),
            count == MAX_STRING_BYTES
        );
    }
    let mut ty = StateType::Boolean;
    for _ in 0..32 {
        ty = StateType::Nullable { of: Box::new(ty) };
    }
    assert!(
        StateContract::checked(
            StateFields::from([("x".into(), ty)]),
            &value(json!({"x":null})),
            &mut ValueBudget::default()
        )
        .is_err()
    );

    let contract = contract(json!({"x":{"kind":"boolean"}}), json!({"x":false}));
    let mut request = contract.instantiate();
    let original = request.snapshot();
    let mut budget = ValueBudget::default();
    for _ in 0..MAX_NODES - 1 {
        budget.container(1).unwrap();
    }
    assert!(
        request
            .apply_patch(
                HookSlot::RequestBefore,
                &value(json!({"x":true})),
                &mut budget
            )
            .is_err()
    );
    assert!(std::sync::Arc::ptr_eq(&original, &request.snapshot()));
    assert!(budget.container(1).is_err());
}
