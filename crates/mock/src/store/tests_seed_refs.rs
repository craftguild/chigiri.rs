//! Seed snapshot references are typed, source-order independent, and never live relations.

use std::sync::Arc;

use chigiri::core::{Source, Value};

use super::SeedData;
use crate::ir::MockIr;

const SDL: &str = r#"
scalar Data @representation(type:JSON)
type Profile { id:ID! label:String! }
type User { id:ID! tenant:String! handle:String! profile:Profile! }
type Post {
  id:ID!
  tenant:String!
  handle:String!
  author:User! @relation(fields:["tenant","handle"],references:["tenant","handle"])
}
type Copy { id:ID! user:User! users:[User!] raw:Data }
type Query { copies:[Copy!]! }
"#;

fn ir() -> Arc<MockIr> {
    Arc::new(MockIr::parse(vec![Source::new("schema/001.graphql", SDL)]).unwrap())
}

fn embedded(store: &super::Store, model: &str, id: &str, field: &str) -> Value {
    let snapshot = store.snapshot();
    let ty = snapshot.ir().graph().type_id(model).unwrap();
    let model = snapshot.ir().model_for(ty).unwrap();
    let slot = snapshot.ir().models()[model.0].slots[field];
    snapshot
        .record(model, id)
        .unwrap()
        .get(slot)
        .unwrap()
        .clone()
}

#[test]
fn sa_09_forward_composite_reference_projects_a_deterministic_snapshot() {
    let source = r#"{
      "Copy":[{"id":"c","user":{"$ref":{"model":"User","key":{"tenant":"t","handle":"h"}}}}],
      "User":[{"id":"u","tenant":"t","handle":"h","profile":{"id":"p","label":"before"}}]
    }"#;
    let mut store = SeedData::json(source).unwrap().store(ir()).unwrap();
    let expected = embedded(&store, "Copy", "c", "user");
    let ir = store.snapshot().ir().clone();
    let user = ir.model_for(ir.graph().type_id("User").unwrap()).unwrap();
    let mut transaction = store.begin(super::CallScope::MutationRoot).unwrap();
    let patch = [(
        "profile",
        Value::Object(
            [("id", Value::from("p")), ("label", Value::from("after"))]
                .into_iter()
                .collect(),
        ),
    )]
    .into_iter()
    .collect();
    transaction
        .update(user, &crate::store::test_selection::id("u"), &patch)
        .unwrap();
    store.commit(transaction.prepare().unwrap()).unwrap();
    assert_eq!(embedded(&store, "Copy", "c", "user"), expected);
}

#[test]
fn sa_09_reference_failures_are_closed_typed_unambiguous_and_acyclic() {
    let cases = [
        (
            r#"{"Copy":[{"id":"c","user":{"$ref":{"model":"User","key":{"id":"missing"}}}}],"User":[]}"#,
            "missing seed reference target",
        ),
        (
            r#"{"Copy":[{"id":"c","user":{"$ref":{"model":"Profile","key":{"id":"p"}}}}],"Profile":[{"id":"p","label":"x"}]}"#,
            "seed reference model type mismatch",
        ),
        (
            r#"{"Copy":[{"id":"c","user":{"$ref":{"model":"User","key":{"tenant":"t"}}}}],"User":[{"id":"u","tenant":"t","handle":"h","profile":{"id":"p","label":"x"}}]}"#,
            "seed reference key is not declared unique",
        ),
        (
            r#"{"Copy":[{"id":"c","user":{"$ref":{"model":"User","key":{"id":1}}}}],"User":[{"id":"u","tenant":"t","handle":"h","profile":{"id":"p","label":"x"}}]}"#,
            "invalid scalar value",
        ),
        (
            r#"{"Copy":[{"id":"c","user":{"$ref":{"model":"User","key":{"id":"u"}}}}],"User":[{"id":"u","tenant":"t","handle":"a","profile":{"id":"p1","label":"x"}},{"id":"u","tenant":"t","handle":"b","profile":{"id":"p2","label":"y"}}]}"#,
            "ambiguous seed reference target",
        ),
        (
            r#"{"Copy":[{"id":"c","user":{"$ref":{"model":"User","key":{"id":"u"}},"id":"override"}}],"User":[{"id":"u","tenant":"t","handle":"h","profile":{"id":"p","label":"x"}}]}"#,
            "seed reference marker must be closed",
        ),
    ];
    for (source, reason) in cases {
        assert_eq!(
            SeedData::json(source)
                .unwrap()
                .store(ir())
                .err()
                .unwrap()
                .message,
            reason
        );
    }

    let recursive = "type User{id:ID! friend:User} type Query{users:[User!]!}";
    let ir = Arc::new(MockIr::parse(vec![Source::new("schema.graphql", recursive)]).unwrap());
    let source = r#"{"User":[{"id":"a","friend":{"$ref":{"model":"User","key":{"id":"b"}}}},{"id":"b","friend":{"$ref":{"model":"User","key":{"id":"a"}}}}]}"#;
    assert_eq!(
        SeedData::json(source)
            .unwrap()
            .store(ir)
            .err()
            .unwrap()
            .message,
        "cyclic seed reference expansion"
    );
}

#[test]
fn sa_09_reference_expansion_obeys_the_cumulative_work_limit() {
    let seed = super::seed_json::object(
        r#"{"Copy":[{"id":"c","user":{"$ref":{"model":"User","key":{"id":"u"}}},"users":[{"$ref":{"model":"User","key":{"id":"u"}}},{"$ref":{"model":"User","key":{"id":"u"}}}]}],"User":[{"id":"u","tenant":"t","handle":"h","profile":{"id":"p","label":"x"}}]}"#,
    )
    .unwrap();
    let ir = ir();
    let model = ir.model_for(ir.graph().type_id("Copy").unwrap()).unwrap();
    let Value::List(rows) = seed.get("Copy").unwrap() else {
        panic!("Copy seed array required")
    };
    let Value::Object(row) = &rows[0] else {
        panic!("Copy seed object required")
    };
    let mut work = crate::value::work::Work::default();
    work.charge(crate::value::work::MAX_WORK - 1).unwrap();
    let mut budget = crate::value::ValueBudget::with_work(work);
    assert_eq!(
        super::seed_refs::expand(&ir, model, row, &seed, &mut budget)
            .err()
            .unwrap()
            .0,
        "request aggregate limit"
    );
}

#[test]
fn sa_09_large_fan_out_is_rejected_by_the_pre_materialization_budget_reservation() {
    let fields = (0..50)
        .map(|index| format!("f{index}:String!"))
        .collect::<Vec<_>>()
        .join(" ");
    let schema = format!(
        "type Payload{{id:ID! {fields}}} type Copy{{id:ID! values:[Payload!]!}} type Query{{copies:[Copy!]!}}"
    );
    let ir = Arc::new(MockIr::parse(vec![Source::new("schema.graphql", schema)]).unwrap());
    let payload = (0..50)
        .map(|index| format!(r#""f{index}":"{}""#, "x".repeat(4096)))
        .collect::<Vec<_>>()
        .join(",");
    let reference = r#"{"$ref":{"model":"Payload","key":{"id":"p"}}}"#;
    let references = std::iter::repeat_n(reference, 400)
        .collect::<Vec<_>>()
        .join(",");
    let source = format!(
        r#"{{"Payload":[{{"id":"p",{payload}}}],"Copy":[{{"id":"c","values":[{references}]}}]}}"#
    );
    assert!(source.len() < crate::limits::SEED_BYTES);
    assert_eq!(
        SeedData::json(&source)
            .unwrap()
            .store(ir)
            .err()
            .unwrap()
            .message,
        "request aggregate limit"
    );
}

#[test]
fn sa_09_json_scalar_reference_shaped_objects_remain_literal_data() {
    let source = r#"{
      "Copy":[{"id":"c","user":{"$ref":{"model":"User","key":{"id":"u"}}},"raw":{"$ref":{"model":"NotAModel","key":{"id":"x"}}}}],
      "User":[{"id":"u","tenant":"t","handle":"h","profile":{"id":"p","label":"x"}}]
    }"#;
    let store = SeedData::json(source).unwrap().store(ir()).unwrap();
    let Value::Object(raw) = embedded(&store, "Copy", "c", "raw") else {
        panic!("JSON object expected")
    };
    assert!(raw.contains_key("$ref"));
}
