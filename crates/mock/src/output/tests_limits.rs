//! Test output depth, capacity, and the shared budget transferred from read and prepare stages.

use crate::store::test_selection::parent as parent_record;

use chigiri::core::{Object, Value};

use super::tests::{field, model, record, store, value};
use crate::store::CallScope;
use crate::value::{MAX_NODES, ValueBudget};

#[test]
fn ab_09_dto_and_stored_output_limits_remain_resource_failures() {
    let store = store();
    let snapshot = store.snapshot();
    let field = field(snapshot.ir(), "Unit", "payload");
    let oversized = Value::Object(
        [
            ("id", Value::from("payload")),
            (
                "data",
                Value::List(vec![Value::Null; crate::value::MAX_ITEMS + 1]),
            ),
        ]
        .into_iter()
        .collect(),
    );
    for result in [
        super::validate_detailed(&snapshot, field, &oversized, &mut ValueBudget::default()),
        super::validate_stored_detailed(&snapshot, field, &oversized, &mut ValueBudget::default()),
    ] {
        let failure = result.err().expect("oversized output must fail");
        assert!(failure.is_resource_limit());
        let error = crate::luau::ScriptError::output(snapshot.ir(), field, failure);
        assert_eq!(error.phase, "budget");
        assert_eq!(error.cause, "MOCK_RESOURCE_LIMIT");
    }
}

#[test]
fn ab_09_shared_work_cancellation_and_deadline_remain_resource_failures() {
    use crate::limits::request::{BudgetFailure, LimitKind};

    let store = store();
    let snapshot = store.snapshot();
    let field = field(snapshot.ir(), "Unit", "payload");
    let value = value(r#"{"id":"payload"}"#);
    for failure in [
        BudgetFailure::Hard(crate::limits::request::LimitNotice {
            kind: LimitKind::Work,
            observed: crate::api::WORK_LIMIT + 1,
        }),
        BudgetFailure::Cancelled,
        BudgetFailure::Deadline,
    ] {
        for stored in [false, true] {
            let mut budget = ValueBudget::default();
            budget.work.fail_with(failure);
            let result = if stored {
                super::validate_stored_detailed(&snapshot, field, &value, &mut budget)
            } else {
                super::validate_detailed(&snapshot, field, &value, &mut budget)
            };
            let failure = result
                .err()
                .expect("failed shared ledger must reject output");
            assert!(failure.is_resource_limit());
            let error = crate::luau::ScriptError::output(snapshot.ir(), field, failure);
            assert_eq!(error.phase, "budget");
            assert_eq!(error.cause, "MOCK_RESOURCE_LIMIT");
        }
    }
}

#[test]
fn rsm_15_output_depth_list_and_string_limits_are_checked_before_copy() {
    let store = store();
    let snapshot = store.snapshot();
    let dto = field(snapshot.ir(), "Unit", "payload");
    let list = field(snapshot.ir(), "Query", "units");
    assert!(
        snapshot
            .reader()
            .output(list, &Value::List(vec![record(); 1000]))
            .is_ok()
    );
    assert!(
        snapshot
            .reader()
            .output(list, &Value::List(vec![record(); 1001]))
            .is_err()
    );
    for (depth, valid) in [(32, true), (33, false)] {
        let mut nested = Value::Null;
        // Charge one level between the root DTO and data before spending the remainder on scalar JSON.
        for _ in 2..depth {
            nested = Value::List(vec![nested]);
        }
        let payload = Value::Object([("id", "p".into()), ("data", nested)].into_iter().collect());
        assert_eq!(snapshot.reader().output(dto, &payload).is_ok(), valid);
    }
    for (bytes, valid) in [(4096, true), (4097, false)] {
        let payload = Value::Object(
            [
                ("id", "p".into()),
                ("data", Value::String("x".repeat(bytes))),
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(snapshot.reader().output(dto, &payload).is_ok(), valid);
    }
}

#[test]
fn rsm_15_reader_shares_empty_find_and_output_nodes_and_retains_failure() {
    let store = store();
    let snapshot = store.snapshot();
    let mut reader = snapshot.reader();
    let model = snapshot
        .ir()
        .model_for(snapshot.ir().graph().type_id("Elsewhere").unwrap())
        .unwrap();
    let field = field(snapshot.ir(), "Query", "unit");
    // A `find` on the empty model consumes two nodes for options and the result array;
    // use the remaining two nodes for null returns.
    let empty = Object::new();
    for _ in 0..(MAX_NODES / 2 - 1) {
        assert!(reader.find(model, &empty).unwrap().is_empty());
    }
    assert!(reader.output(field, &Value::Null).is_ok());
    assert!(reader.output(field, &Value::Null).is_ok());
    assert!(reader.output(field, &Value::Null).is_err());
    assert!(reader.find(model, &Object::new()).is_err());
}

#[test]
fn rsm_15_prepared_carries_the_host_value_budget_into_return_validation() {
    let mut store = store();
    let snapshot = store.snapshot();
    let model = snapshot
        .ir()
        .model_for(snapshot.ir().graph().type_id("Elsewhere").unwrap())
        .unwrap();
    let field = field(snapshot.ir(), "Query", "unit");
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    let options = Object::new();
    for _ in 0..(MAX_NODES / 2) {
        tx.find(model, &options).unwrap();
    }
    let mut prepared = tx.prepare().unwrap();
    assert!(prepared.output(field, &Value::Null).is_err());
    assert!(store.commit(prepared).is_err());
}

#[test]
fn rsm_15_output_counts_json_keys_null_and_typename_in_shared_bytes() {
    let store = store();
    let snapshot = store.snapshot();
    let field = field(snapshot.ir(), "Unit", "payload");
    let raw = value(r#"{"__typename":"Payload","id":"p","data":{"key":null}}"#);
    let mut budget = ValueBudget::default();
    super::validate(&snapshot, field, &raw, &mut budget).unwrap();
    assert_eq!(budget.nodes(), 5);
    assert_eq!(budget.bytes(), 30);
    let mut budget = ValueBudget::default();
    // A rejected byte pre-reservation still charges the corresponding work without increasing byte usage.
    assert!(budget.key_bytes(crate::api::WORK_LIMIT).is_err());
    assert_eq!(budget.bytes(), 0);
    assert!(super::validate(&snapshot, field, &raw, &mut budget).is_err());
    assert!(super::validate(&snapshot, field, &Value::Null, &mut budget).is_err());
    assert!(super::validate(&snapshot, field, &raw, &mut ValueBudget::default()).is_ok());
}

#[test]
fn rsm_15_reader_charges_empty_relation_lookups_and_sticks_after_work_limit() {
    let store = store();
    let snapshot = store.snapshot();
    let relation = field(snapshot.ir(), "Unit", "next");
    let output = field(snapshot.ir(), "Query", "unit");
    let model = model(snapshot.ir());
    let mut reader = snapshot.reader();
    let parent = parent_record(&snapshot, relation, "u");
    // Exercise parent encoding, row reads, foreign-key components, and empty tuples independently.
    for _ in 0..(crate::api::WORK_LIMIT / 4) {
        assert!(matches!(
            reader.related(relation, &parent).unwrap(),
            crate::store::Related::Null
        ));
    }
    assert!(reader.related(relation, &parent).is_err());
    assert!(reader.find(model, &Object::new()).is_err());
    assert!(reader.output(output, &Value::Null).is_err());
}
